use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use redis::{Client, Commands, RedisError};
use tokio::sync::RwLock;
use unleash_types::client_features::ClientFeatures;

pub const FEATURES_KEY: &str = "unleash-features";
pub const TOKENS_KEY: &str = "unleash-tokens";
pub const REFRESH_TARGETS_KEY: &str = "unleash-refresh-targets";

use crate::types::{EdgeToken, TokenRefresh};
use crate::{error::EdgeError, types::EdgeResult};

use super::EdgePersistence;

impl From<RedisError> for EdgeError {
    fn from(err: RedisError) -> Self {
        EdgeError::PersistenceError(format!("Error connecting to Redis: {err}"))
    }
}

pub struct RedisPersister {
    redis_client: Arc<RwLock<Client>>,
}

impl RedisPersister {
    pub fn new(url: &str) -> Result<RedisPersister, EdgeError> {
        let client = Arc::new(RwLock::new(redis::Client::open(url)?));

        Ok(Self {
            redis_client: client,
        })
    }
}

#[async_trait]
impl EdgePersistence for RedisPersister {
    async fn load_tokens(&self) -> EdgeResult<Vec<EdgeToken>> {
        let mut client = self.redis_client.write().await;
        let raw_tokens: String = client.get(TOKENS_KEY)?;
        serde_json::from_str::<Vec<EdgeToken>>(&raw_tokens)
            .map_err(|_e| EdgeError::TokenParseError("Failed to load tokens from redis".into()))
    }

    async fn save_tokens(&self, tokens: Vec<EdgeToken>) -> EdgeResult<()> {
        let mut client = self.redis_client.write().await;
        let raw_tokens = serde_json::to_string(&tokens)?;
        client.set(TOKENS_KEY, raw_tokens)?;
        Ok(())
    }

    async fn load_refresh_targets(&self) -> EdgeResult<Vec<TokenRefresh>> {
        let mut client = self.redis_client.write().await;
        let refresh_targets: String = client.get(REFRESH_TARGETS_KEY)?;
        serde_json::from_str::<Vec<TokenRefresh>>(&refresh_targets).map_err(|_| {
            EdgeError::TokenParseError("Failed to load refresh targets from redis".into())
        })
    }

    async fn save_refresh_targets(&self, refresh_targets: Vec<TokenRefresh>) -> EdgeResult<()> {
        let mut client = self.redis_client.write().await;
        let refresh_targets = serde_json::to_string(&refresh_targets)?;
        client.set(REFRESH_TARGETS_KEY, refresh_targets)?;
        Ok(())
    }

    async fn load_features(&self) -> EdgeResult<HashMap<String, ClientFeatures>> {
        let mut client = self.redis_client.write().await;
        let raw_features: String = client.get(FEATURES_KEY)?;
        let raw_features = serde_json::from_str::<Vec<(String, ClientFeatures)>>(&raw_features)
            .map_err(|_| EdgeError::ClientFeaturesParseError)?;
        Ok(raw_features.into_iter().collect())
    }

    async fn save_features(&self, features: Vec<(String, ClientFeatures)>) -> EdgeResult<()> {
        let mut client = self.redis_client.write().await;
        let raw_features = serde_json::to_string(&features)?;
        client
            .set(FEATURES_KEY, raw_features)
            .map_err(EdgeError::from)
    }
}
