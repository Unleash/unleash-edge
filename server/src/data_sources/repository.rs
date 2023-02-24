use std::sync::Arc;

use actix_web::http::header::EntityTag;
use async_trait::async_trait;
use chrono::{Duration, Utc};
use unleash_types::client_features::{ClientFeature, ClientFeatures};

use crate::{
    error::EdgeError,
    types::{
        EdgeResult, EdgeSink, EdgeSource, EdgeToken, FeatureSink, FeatureSource, TokenRefresh,
        TokenSink, TokenSource, TokenType, TokenValidationStatus,
    },
};

trait ProjectFilter<T> {
    fn filter_by_projects(&self, token: &EdgeToken) -> Vec<T>;
}

impl ProjectFilter<ClientFeature> for Vec<ClientFeature> {
    fn filter_by_projects(&self, token: &EdgeToken) -> Vec<ClientFeature> {
        self.iter()
            .filter(|feature| {
                if let Some(feature_project) = &feature.project {
                    token.projects.contains(&"*".to_string())
                        || token.projects.contains(feature_project)
                } else {
                    false
                }
            })
            .cloned()
            .collect::<Vec<ClientFeature>>()
    }
}

#[derive(Clone)]
pub struct DataSourceFacade {
    pub(crate) features_refresh_interval: Option<Duration>,
    pub(crate) token_source: Arc<dyn DataSource>,
    pub(crate) feature_source: Arc<dyn DataSource>,
    pub token_sink: Arc<dyn DataSink>,
    pub feature_sink: Arc<dyn DataSink>,
}

impl EdgeSource for DataSourceFacade {}
impl EdgeSink for DataSourceFacade {}

#[async_trait]
pub trait DataSource: Send + Sync {
    async fn get_tokens(&self) -> EdgeResult<Vec<EdgeToken>>;
    async fn get_token(&self, secret: &str) -> EdgeResult<Option<EdgeToken>>;
    async fn get_refresh_tokens(&self) -> EdgeResult<Vec<TokenRefresh>>;
    async fn get_client_features(&self, token: &EdgeToken) -> EdgeResult<Option<ClientFeatures>>;
}

#[async_trait]
pub trait DataSink: Send + Sync {
    async fn sink_tokens(&self, tokens: Vec<EdgeToken>) -> EdgeResult<()>;
    async fn set_refresh_tokens(&self, tokens: Vec<&TokenRefresh>) -> EdgeResult<()>;
    async fn sink_features(&self, token: &EdgeToken, features: ClientFeatures) -> EdgeResult<()>;
    async fn update_last_check(&self, token: &EdgeToken) -> EdgeResult<()>;
    async fn update_last_refresh(
        &self,
        token: &EdgeToken,
        etag: Option<EntityTag>,
    ) -> EdgeResult<()>;
}

#[async_trait]
impl TokenSource for DataSourceFacade {
    async fn get_tokens(&self) -> EdgeResult<Vec<EdgeToken>> {
        self.token_source.get_tokens().await
    }

    async fn get_valid_tokens(&self) -> EdgeResult<Vec<EdgeToken>> {
        self.token_source.get_tokens().await.map(|result| {
            result
                .iter()
                .filter(|t| t.status == TokenValidationStatus::Validated)
                .cloned()
                .collect()
        })
    }

    async fn get_token(&self, secret: String) -> EdgeResult<Option<EdgeToken>> {
        self.token_source.get_token(secret.as_str()).await
    }

    async fn filter_valid_tokens(&self, tokens: Vec<String>) -> EdgeResult<Vec<EdgeToken>> {
        let mut known_tokens = self.token_source.get_tokens().await?;
        known_tokens.retain(|t| tokens.contains(&t.token));
        Ok(known_tokens)
    }

    async fn get_tokens_due_for_refresh(&self) -> EdgeResult<Vec<TokenRefresh>> {
        let refresh_tokens = self.token_source.get_refresh_tokens().await?;

        let refresh_interval = self
            .features_refresh_interval
            .ok_or_else(|| EdgeError::DataSourceError("No refresh interval set".into()))?;

        Ok(refresh_tokens
            .iter()
            .filter(|token| {
                token
                    .last_check
                    .map(|last| Utc::now() - last > refresh_interval)
                    .unwrap_or(true)
            })
            .cloned()
            .collect())
    }
}

#[async_trait]
impl FeatureSource for DataSourceFacade {
    async fn get_client_features(&self, token: &EdgeToken) -> EdgeResult<ClientFeatures> {
        let token = self
            .get_token(token.token.clone())
            .await?
            .unwrap_or_else(|| token.clone());

        let environment_features = self.feature_source.get_client_features(&token).await?;

        Ok(environment_features
            .map(|client_features| ClientFeatures {
                features: client_features.features.filter_by_projects(&token),
                ..client_features
            })
            .ok_or_else(|| EdgeError::DataSourceError("No features found".into()))?)
    }
}

#[async_trait]
impl TokenSink for DataSourceFacade {
    async fn sink_tokens(&self, tokens: Vec<EdgeToken>) -> EdgeResult<()> {
        self.token_sink.sink_tokens(tokens.clone()).await?;

        let refresh_tokens = tokens
            .into_iter()
            .filter(|t| t.token_type == Some(TokenType::Client))
            .map(TokenRefresh::new);

        let current_refresh_tokens: Vec<TokenRefresh> = self
            .token_source
            .get_refresh_tokens()
            .await?
            .into_iter()
            .chain(refresh_tokens)
            .collect();

        let reduced_refresh_tokens = crate::tokens::simplify(&current_refresh_tokens);

        self.token_sink
            .set_refresh_tokens(reduced_refresh_tokens)
            .await
    }
}

#[async_trait]
impl FeatureSink for DataSourceFacade {
    async fn sink_features(&self, token: &EdgeToken, features: ClientFeatures) -> EdgeResult<()> {
        self.feature_sink.sink_features(token, features).await?;
        Ok(())
    }

    async fn update_last_check(&self, token: &EdgeToken) -> EdgeResult<()> {
        self.feature_sink.update_last_check(token).await?;
        Ok(())
    }

    async fn update_last_refresh(
        &self,
        token: &EdgeToken,
        etag: Option<EntityTag>,
    ) -> EdgeResult<()> {
        self.feature_sink.update_last_refresh(token, etag).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{str::FromStr, sync::Arc};

    use chrono::Duration;
    use unleash_types::client_features::{ClientFeature, ClientFeatures};

    use crate::{
        data_sources::memory_provider::MemoryProvider,
        types::{EdgeResult, EdgeSink, EdgeSource, EdgeToken, TokenType, TokenValidationStatus},
    };

    use super::DataSourceFacade;

    fn build_data_source() -> EdgeResult<(Arc<dyn EdgeSource>, Arc<dyn EdgeSink>)> {
        let data_store = Arc::new(MemoryProvider::new());
        let facade = Arc::new(DataSourceFacade {
            token_source: data_store.clone(),
            feature_source: data_store.clone(),
            token_sink: data_store.clone(),
            feature_sink: data_store,
            features_refresh_interval: Some(Duration::minutes(1)),
        });
        let source: Arc<dyn EdgeSource> = facade.clone();
        let sink: Arc<dyn EdgeSink> = facade;

        Ok((source, sink))
    }

    #[tokio::test]
    async fn sinking_tokens_only_saves_a_minimal_set_of_refresh_tokens() {
        let (source, sink) = build_data_source().unwrap();
        let mut default_development_token =
            EdgeToken::from_str("default:development.1234567890123456").unwrap();
        default_development_token.token_type = Some(TokenType::Client);
        let mut test_development_token =
            EdgeToken::from_str("test:development.abcdefghijklmnopqerst").unwrap();
        test_development_token.token_type = Some(TokenType::Client);

        sink.sink_tokens(vec![default_development_token, test_development_token])
            .await
            .unwrap();

        let tokens_due_for_refresh = source.get_tokens_due_for_refresh().await.unwrap();

        assert_eq!(tokens_due_for_refresh.len(), 2);

        let mut wildcard_token =
            EdgeToken::from_str("*:development.12321jwewhrkvkjewlrkjwqlkrjw").unwrap();
        wildcard_token.token_type = Some(TokenType::Client);

        sink.sink_tokens(vec![wildcard_token.clone()])
            .await
            .unwrap();

        let tokens_due_for_refresh = source.get_tokens_due_for_refresh().await.unwrap();

        assert_eq!(tokens_due_for_refresh.len(), 1);
    }

    #[tokio::test]
    async fn filters_out_features_by_token() {
        let (source, sink) = build_data_source().unwrap();
        let token = EdgeToken {
            status: TokenValidationStatus::Validated,
            environment: Some("some-env-1".to_string()),
            projects: vec!["default".to_string()],
            ..EdgeToken::default()
        };

        let mock_features = ClientFeatures {
            features: vec![
                ClientFeature {
                    name: "default".into(),
                    project: Some("default".into()),
                    ..ClientFeature::default()
                },
                ClientFeature {
                    name: "fancy".into(),
                    project: Some("fancy".into()),
                    ..ClientFeature::default()
                },
            ],
            version: 2,
            query: None,
            segments: None,
        };

        let expected = ClientFeatures {
            features: vec![ClientFeature {
                name: "default".into(),
                project: Some("default".into()),
                ..ClientFeature::default()
            }],
            version: 2,
            query: None,
            segments: None,
        };

        sink.sink_features(&token, mock_features).await.unwrap();

        let response = source.get_client_features(&token).await.unwrap();
        assert_eq!(response, expected);
    }

    #[tokio::test]
    async fn getting_features_for_token_returns_all_features_when_token_is_all_projects() {}

    #[tokio::test]
    async fn getting_features_for_token_returns_only_project_features_when_token_has_project_set() {
    }
}
