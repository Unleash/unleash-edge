use std::sync::Arc;

use actix_web::http::header::EntityTag;
use async_trait::async_trait;
use chrono::{Duration, Utc};
use tokio::sync::RwLock;
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
pub struct SourceFacade {
    pub(crate) features_refresh_interval: Option<Duration>,
    pub(crate) token_source: Arc<RwLock<dyn DataSource>>,
    pub(crate) feature_source: Arc<RwLock<dyn DataSource>>,
}

#[derive(Clone)]
pub struct SinkFacade {
    pub token_sink: Arc<RwLock<dyn DataSink>>,
    pub feature_sink: Arc<RwLock<dyn DataSink>>,
}

impl EdgeSource for SourceFacade {}
impl EdgeSink for SinkFacade {}

#[async_trait]
pub trait DataSource: Send + Sync {
    async fn get_tokens(&self) -> EdgeResult<Vec<EdgeToken>>;
    async fn get_token(&self, secret: &str) -> EdgeResult<Option<EdgeToken>>;
    async fn get_refresh_tokens(&self) -> EdgeResult<Vec<TokenRefresh>>;
    async fn get_client_features(&self, token: &EdgeToken) -> EdgeResult<Option<ClientFeatures>>;
}

#[async_trait]
pub trait DataSink: Send + Sync {
    async fn sink_tokens(&mut self, tokens: Vec<EdgeToken>) -> EdgeResult<()>;
    async fn sink_refresh_tokens(&mut self, tokens: Vec<&TokenRefresh>) -> EdgeResult<()>;
    async fn sink_features(
        &mut self,
        token: &EdgeToken,
        features: ClientFeatures,
    ) -> EdgeResult<()>;
    async fn update_last_check(&mut self, token: &EdgeToken) -> EdgeResult<()>;
    async fn update_last_refresh(
        &mut self,
        token: &EdgeToken,
        etag: Option<EntityTag>,
    ) -> EdgeResult<()>;
}

#[async_trait]
impl TokenSource for SourceFacade {
    async fn get_tokens(&self) -> EdgeResult<Vec<EdgeToken>> {
        let lock = self.token_source.read().await;
        lock.get_tokens().await
    }

    async fn get_valid_tokens(&self) -> EdgeResult<Vec<EdgeToken>> {
        let lock = self.token_source.read().await;
        lock.get_tokens().await.map(|result| {
            result
                .iter()
                .filter(|t| t.status == TokenValidationStatus::Validated)
                .cloned()
                .collect()
        })
    }

    async fn get_token(&self, secret: String) -> EdgeResult<Option<EdgeToken>> {
        let lock = self.token_source.read().await;
        lock.get_token(secret.as_str()).await
    }

    async fn filter_valid_tokens(&self, tokens: Vec<String>) -> EdgeResult<Vec<EdgeToken>> {
        let mut known_tokens = self.token_source.read().await.get_tokens().await?;
        known_tokens.retain(|t| tokens.contains(&t.token));
        Ok(known_tokens)
    }

    async fn get_tokens_due_for_refresh(&self) -> EdgeResult<Vec<TokenRefresh>> {
        let lock = self.token_source.read().await;
        let refresh_tokens = lock.get_refresh_tokens().await?;

        let refresh_interval = self
            .features_refresh_interval
            .ok_or(EdgeError::DataSourceError("No refresh interval set".into()))?;

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
impl FeatureSource for SourceFacade {
    async fn get_client_features(&self, token: &EdgeToken) -> EdgeResult<ClientFeatures> {
        let token = self
            .get_token(token.token.clone())
            .await?
            .unwrap_or(token.clone());

        let environment_features = self
            .feature_source
            .read()
            .await
            .get_client_features(&token)
            .await
            .unwrap();

        Ok(environment_features
            .map(|client_features| ClientFeatures {
                features: client_features.features.filter_by_projects(&token),
                ..client_features
            })
            .unwrap())
    }
}

#[async_trait]
impl TokenSink for SinkFacade {
    async fn sink_tokens(&self, tokens: Vec<EdgeToken>) -> EdgeResult<()> {
        let mut lock = self.token_sink.write().await;
        lock.sink_tokens(tokens.clone()).await?;

        let refresh_tokens: Vec<TokenRefresh> = tokens
            .into_iter()
            .filter(|t| t.token_type == Some(TokenType::Client))
            .map(TokenRefresh::new)
            .collect();

        let reduced_refresh_tokens = crate::tokens::simplify(&refresh_tokens);

        lock.sink_refresh_tokens(reduced_refresh_tokens).await
    }
}

#[async_trait]
impl FeatureSink for SinkFacade {
    async fn sink_features(&self, token: &EdgeToken, features: ClientFeatures) -> EdgeResult<()> {
        let mut lock = self.feature_sink.write().await;

        lock.sink_features(token, features).await?;

        Ok(())
    }

    async fn update_last_check(&self, token: &EdgeToken) -> EdgeResult<()> {
        let mut lock = self.feature_sink.write().await;
        lock.update_last_check(token).await?;
        Ok(())
    }

    async fn update_last_refresh(
        &self,
        token: &EdgeToken,
        etag: Option<EntityTag>,
    ) -> EdgeResult<()> {
        let mut lock = self.feature_sink.write().await;
        lock.update_last_refresh(token, etag).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{str::FromStr, sync::Arc};

    use chrono::Duration;
    use tokio::sync::RwLock;
    use unleash_types::client_features::{ClientFeature, ClientFeatures};

    use crate::{
        client_api::features,
        data_sources::memory_provider::MemoryProvider,
        types::{EdgeResult, EdgeSink, EdgeSource, EdgeToken, TokenType, TokenValidationStatus},
    };

    use super::{SinkFacade, SourceFacade};

    fn build_data_source() -> EdgeResult<(Arc<dyn EdgeSource>, Arc<dyn EdgeSink>)> {
        // It would be really nice if we could run all these tests against all the data sources automagically
        let data_store = Arc::new(RwLock::new(MemoryProvider::new()));
        let source: Arc<dyn EdgeSource> = Arc::new(SourceFacade {
            token_source: data_store.clone(),
            feature_source: data_store.clone(),
            features_refresh_interval: Some(Duration::minutes(1)),
        });
        let sink: Arc<dyn EdgeSink> = Arc::new(SinkFacade {
            token_sink: data_store.clone(),
            feature_sink: data_store.clone(),
        });
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
