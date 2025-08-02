use ahash::HashMap;
use async_trait::async_trait;
use dashmap::DashMap;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, warn};
use unleash_edge_feature_cache::FeatureCache;
use unleash_edge_types::tokens::EdgeToken;
use unleash_edge_types::{EdgeResult, TokenValidationStatus};
use unleash_types::client_features::ClientFeatures;

pub mod file;
pub mod redis;
pub mod s3;

#[async_trait]
pub trait EdgePersistence: Send + Sync {
    async fn load_tokens(&self) -> EdgeResult<Vec<EdgeToken>>;
    async fn save_tokens(&self, tokens: Vec<EdgeToken>) -> EdgeResult<()>;
    async fn load_features(&self) -> EdgeResult<HashMap<String, ClientFeatures>>;
    async fn save_features(&self, features: Vec<(String, ClientFeatures)>) -> EdgeResult<()>;
}

pub async fn persist_data(
    persistence: Option<Arc<dyn EdgePersistence>>,
    token_cache: Arc<DashMap<String, EdgeToken>>,
    features_cache: Arc<FeatureCache>,
) {
    loop {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(60)) => {
                if let Some(persister) = persistence.clone() {

                    save_known_tokens(&token_cache, &persister).await;
                    save_features(&features_cache, &persister).await;
                } else {
                    debug!("No persistence configured, skipping persistence");
                }
            }
        }
    }
}

async fn save_known_tokens(
    token_cache: &Arc<DashMap<String, EdgeToken>>,
    persister: &Arc<dyn EdgePersistence>,
) {
    if !token_cache.is_empty() {
        match persister
            .save_tokens(
                token_cache
                    .iter()
                    .filter(|t| t.value().status == TokenValidationStatus::Validated)
                    .map(|e| e.value().clone())
                    .collect(),
            )
            .await
        {
            Ok(()) => debug!("Persisted tokens"),
            Err(save_error) => warn!("Could not persist tokens: {save_error:?}"),
        }
    } else {
        debug!("No validated tokens found, skipping tokens persistence");
    }
}

async fn save_features(features_cache: &FeatureCache, persister: &Arc<dyn EdgePersistence>) {
    if !features_cache.is_empty() {
        match persister
            .save_features(
                features_cache
                    .iter()
                    .map(|e| (e.key().clone(), e.value().clone()))
                    .collect(),
            )
            .await
        {
            Ok(()) => debug!("Persisted features"),
            Err(save_error) => warn!("Could not persist features: {save_error:?}"),
        }
    } else {
        debug!("No features found, skipping features persistence");
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    struct MockPersistence {}

    fn build_mock_persistence() -> Arc<dyn EdgePersistence> {
        Arc::new(MockPersistence {})
    }

    #[async_trait]
    impl EdgePersistence for MockPersistence {
        async fn load_tokens(&self) -> EdgeResult<Vec<EdgeToken>> {
            panic!("Not expected to be called");
        }

        async fn save_tokens(&self, _: Vec<EdgeToken>) -> EdgeResult<()> {
            panic!("Not expected to be called");
        }

        async fn load_features(&self) -> EdgeResult<HashMap<String, ClientFeatures>> {
            panic!("Not expected to be called");
        }

        async fn save_features(&self, _: Vec<(String, ClientFeatures)>) -> EdgeResult<()> {
            panic!("Not expected to be called");
        }
    }

    #[tokio::test]
    async fn persistence_ignores_empty_feature_sets() {
        let cache: DashMap<String, ClientFeatures> = DashMap::new();
        let persister = build_mock_persistence();

        save_features(&Arc::new(FeatureCache::new(cache)), &persister.clone()).await;
    }

    #[tokio::test]
    async fn persistence_ignores_empty_token_sets() {
        let cache: DashMap<String, EdgeToken> = DashMap::new();
        let persister = build_mock_persistence();

        save_known_tokens(&Arc::new(cache), &persister.clone()).await;
    }
}
