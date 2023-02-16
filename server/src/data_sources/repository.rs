use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;
use unleash_types::client_features::ClientFeatures;

use crate::types::{
    EdgeResult, EdgeSource, EdgeToken, FeatureRefresh, FeaturesSource, TokenSource,
    TokenValidationStatus,
};

#[derive(Clone)]
pub struct SourceFacade {
    toggle_source: Arc<RwLock<dyn TokenSource>>,
    feature_source: Arc<RwLock<dyn FeaturesSource>>,
}

#[async_trait]
pub trait DataSource: Send + Sync {
    async fn get_tokens(&self) -> EdgeResult<Vec<EdgeToken>>;
    async fn get_token(&self, secret: &str) -> EdgeResult<Option<EdgeToken>>;
    async fn get_tokens_due_for_refresh(&self) -> EdgeResult<Vec<FeatureRefresh>>;
    async fn get_client_features(&self, token: &EdgeToken) -> EdgeResult<ClientFeatures>;
}

impl EdgeSource for SourceFacade {}

#[async_trait]
impl TokenSource for SourceFacade {
    async fn get_known_tokens(&self) -> EdgeResult<Vec<EdgeToken>> {
        let lock = self.source.read().await;
        lock.get_tokens().await
    }

    async fn get_valid_tokens(&self) -> EdgeResult<Vec<EdgeToken>> {
        let lock = self.source.read().await;
        lock.get_tokens().await.map(|result| {
            result
                .iter()
                .filter(|t| t.status == TokenValidationStatus::Validated)
                .cloned()
                .collect()
        })
    }

    async fn token_details(&self, secret: String) -> EdgeResult<Option<EdgeToken>> {
        let lock = self.source.read().await;
        lock.get_token(secret.as_str()).await
    }

    async fn filter_valid_tokens(&self, tokens: Vec<String>) -> EdgeResult<Vec<EdgeToken>> {
        let lock = self.source.read().await;
        let mut known_tokens = lock.get_tokens().await?;
        drop(lock);
        known_tokens.retain(|t| tokens.contains(&t.token));
        Ok(known_tokens)
    }

    async fn get_tokens_due_for_refresh(&self) -> EdgeResult<Vec<FeatureRefresh>> {
        let lock = self.source.read().await;
        lock.get_tokens_due_for_refresh().await
    }
}

#[async_trait]
impl FeaturesSource for SourceFacade {
    async fn get_client_features(&self, token: &EdgeToken) -> EdgeResult<ClientFeatures> {
        let lock = self.source.read().await;
        lock.get_client_features(token).await
    }
}
