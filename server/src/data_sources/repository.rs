use std::sync::Arc;

use actix_web::http::header::EntityTag;
use async_trait::async_trait;
use chrono::{Duration, Utc};
use tokio::sync::RwLock;
use unleash_types::client_features::{ClientFeature, ClientFeatures};

use crate::{
    error::EdgeError,
    types::{
        EdgeResult, EdgeSink, EdgeSource, EdgeToken, TokenRefresh, FeatureSource,
        TokenSink, TokenSource, TokenValidationStatus, FeatureSink,
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
    async fn update_last_refresh(&mut self, token: &EdgeToken, etag: Option<EntityTag>) -> EdgeResult<()>;
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

        let refresh_tokens: Vec<TokenRefresh> =
            tokens.into_iter().map(TokenRefresh::new).collect();

        let reduced_refresh_tokens = crate::tokens::simplify(&refresh_tokens);

        lock.sink_refresh_tokens(reduced_refresh_tokens).await
    }
}

#[async_trait]
impl FeatureSink for SinkFacade {
    async fn sink_features(
        &self,
        token: &EdgeToken,
        features: ClientFeatures
    ) -> EdgeResult<()> {
        let mut lock = self.feature_sink.write().await;

        lock.sink_features(token, features).await?;

        Ok(())
    }
    async fn update_last_check(&self, token: &EdgeToken) -> EdgeResult<()> {
        let mut lock = self.feature_sink.write().await;
        lock.update_last_check(token).await?;
        Ok(())
    }
    async fn update_last_refresh(&self, token: &EdgeToken, etag: Option<EntityTag>) -> EdgeResult<()> {
        let mut lock = self.feature_sink.write().await;
        lock.update_last_refresh(token, etag).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {

    #[tokio::test]
    pub async fn when_sinking_tokens_also_creates_minimal_tokens() {

        // let mut memory_provider = MemoryProvider::default();
        // let mut token_for_default_project =
        //     EdgeToken::try_from("default:development.1234567890123456".to_string()).unwrap();
        // token_for_default_project.token_type = Some(TokenType::Client);

        // let mut token_for_test_project =
        //     EdgeToken::try_from("test:development.abcdefghijklmnopqerst".to_string()).unwrap();

        // token_for_test_project.token_type = Some(TokenType::Client);
        // memory_provider
        //     .sink_tokens(vec![token_for_test_project, token_for_default_project])
        //     .await
        //     .unwrap();

        // assert_eq!(memory_provider.tokens_to_refresh.len(), 2);

        // let mut wildcard_development_token =
        //     EdgeToken::try_from("*:development.12321jwewhrkvkjewlrkjwqlkrjw".to_string()).unwrap();

        // wildcard_development_token.token_type = Some(TokenType::Client);

        // memory_provider
        //     .sink_tokens(vec![wildcard_development_token.clone()])
        //     .await
        //     .unwrap();

        // assert_eq!(memory_provider.tokens_to_refresh.len(), 1);
        // assert!(memory_provider
        //     .tokens_to_refresh
        //     .contains_key(&wildcard_development_token.token));
    }
}
