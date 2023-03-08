use std::{collections::HashMap, sync::Arc, time::Duration};

use async_trait::async_trait;
use dashmap::DashMap;
use tracing::{debug, warn};
use unleash_types::client_features::ClientFeatures;

use crate::types::{EdgeResult, EdgeToken, TokenRefresh};

pub mod file;
pub mod redis;

#[async_trait]
pub trait EdgePersistence: Send + Sync {
    async fn load_tokens(&self) -> EdgeResult<Vec<EdgeToken>>;
    async fn save_tokens(&self, tokens: Vec<EdgeToken>) -> EdgeResult<()>;
    async fn load_refresh_targets(&self) -> EdgeResult<Vec<TokenRefresh>>;
    async fn save_refresh_targets(&self, refresh_targets: Vec<TokenRefresh>) -> EdgeResult<()>;
    async fn load_features(&self) -> EdgeResult<HashMap<String, ClientFeatures>>;
    async fn save_features(&self, features: Vec<(String, ClientFeatures)>) -> EdgeResult<()>;
}

#[cfg(not(tarpaulin_include))]
pub async fn persist_data(
    persistence: Option<Arc<dyn EdgePersistence>>,
    token_cache: Arc<DashMap<String, EdgeToken>>,
    features_cache: Arc<DashMap<String, ClientFeatures>>,
    refresh_targets_cache: Arc<DashMap<String, TokenRefresh>>,
) {
    loop {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(60)) => {
                if let Some(persister) = persistence.clone() {
                    if persister.save_tokens(token_cache.iter().map(|e| e.value().clone()).collect()).await.is_ok() {
                        debug!("Persisted tokens");
                    } else {
                        warn!("Could not persist tokens");
                    }
                    if persister.save_features(features_cache.iter().map(|e| (e.key().clone(), e.value().clone())).collect()).await.is_ok() {
                        debug!("Persisted features");
                    }else {
                        warn!("Could not persist features");
                    }

                    if persister.save_refresh_targets(refresh_targets_cache.iter().map(|e| e.value().clone()).collect()).await.is_ok() {
                        debug!("Persisted refresh targets");
                    } else {
                        warn!("Could not persist refresh targets");
                    }
                } else {
                    debug!("Had no persister. Nothing was persisted");
                }
            }
        }
    }
}
