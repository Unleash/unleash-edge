use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use redis::{Client, Commands, RedisError};
use redis::cluster::ClusterClient;
use tokio::sync::RwLock;
use unleash_types::client_features::ClientFeatures;

use crate::{error::EdgeError, types::EdgeResult};
use crate::persistence::redis::RedisClientOptions::{Cluster, Single};
use crate::types::{EdgeToken, TokenRefresh};

use super::EdgePersistence;

pub const FEATURES_KEY: &str = "unleash-features";
pub const TOKENS_KEY: &str = "unleash-tokens";
pub const REFRESH_TARGETS_KEY: &str = "unleash-refresh-targets";

impl From<RedisError> for EdgeError {
    fn from(err: RedisError) -> Self {
        EdgeError::PersistenceError(format!("Error connecting to Redis: {err}"))
    }
}

enum RedisClientOptions {
    Single(Client),
    Cluster(ClusterClient),
}

pub struct RedisPersister {
    redis_client: Arc<RwLock<RedisClientOptions>>,
}

impl RedisPersister {
    pub fn new(url: &str) -> Result<RedisPersister, EdgeError> {
        let client = Client::open(url)?;

        Ok(Self {
            redis_client: Arc::new(RwLock::new(Single(client))),
        })
    }
    pub fn new_with_cluster(urls: Vec<String>) -> Result<RedisPersister, EdgeError> {
        let client = ClusterClient::new(urls)?;
        Ok(Self {
            redis_client: Arc::new(RwLock::new(Cluster(client))),
        })
    }
}

#[async_trait]
impl EdgePersistence for RedisPersister {
    async fn load_tokens(&self) -> EdgeResult<Vec<EdgeToken>> {
        let mut client = self.redis_client.write().await;
        let raw_tokens: String = match &mut *client {
            Single(c) => c.get(TOKENS_KEY)?,
            Cluster(c) => {
                let mut conn = c.get_connection()?;
                conn.get(TOKENS_KEY)?
            }
        };
        serde_json::from_str::<Vec<EdgeToken>>(&raw_tokens)
            .map_err(|_e| EdgeError::TokenParseError("Failed to load tokens from redis".into()))
    }

    async fn save_tokens(&self, tokens: Vec<EdgeToken>) -> EdgeResult<()> {
        let mut client = self.redis_client.write().await;
        let raw_tokens = serde_json::to_string(&tokens)?;
        match &mut *client {
            RedisClientOptions::Single(c) => c.set(TOKENS_KEY, raw_tokens)?,
            RedisClientOptions::Cluster(c) => {
                let mut conn = c.get_connection()?;
                conn.set(TOKENS_KEY, raw_tokens)?
            }
        };
        Ok(())
    }

    async fn load_refresh_targets(&self) -> EdgeResult<Vec<TokenRefresh>> {
        let mut client = self.redis_client.write().await;
        let refresh_targets: String = match &mut *client {
            Single(client) => client.get(REFRESH_TARGETS_KEY)?,
            Cluster(client) => {
                let mut conn = client.get_connection()?;
                conn.get(REFRESH_TARGETS_KEY)?
            }
        };
        serde_json::from_str::<Vec<TokenRefresh>>(&refresh_targets).map_err(|_| {
            EdgeError::TokenParseError("Failed to load refresh targets from redis".into())
        })
    }

    async fn save_refresh_targets(&self, refresh_targets: Vec<TokenRefresh>) -> EdgeResult<()> {
        let mut client = self.redis_client.write().await;
        let refresh_targets = serde_json::to_string(&refresh_targets)?;
        match &mut *client {
            Single(client) => client.set(REFRESH_TARGETS_KEY, refresh_targets)?,
            Cluster(client) => {
                let mut conn = client.get_connection()?;
                conn.set(REFRESH_TARGETS_KEY, refresh_targets)?
            }
        };
        Ok(())
    }

    async fn load_features(&self) -> EdgeResult<HashMap<String, ClientFeatures>> {
        let mut client = self.redis_client.write().await;
        let raw_features: String = match &mut *client {
            Single(client) => client.get(FEATURES_KEY)?,
            Cluster(client) => {
                let mut conn = client.get_connection()?;
                conn.get(FEATURES_KEY)?
            }
        };
        let raw_features = serde_json::from_str::<Vec<(String, ClientFeatures)>>(&raw_features)
            .map_err(|e| EdgeError::ClientFeaturesParseError(e.to_string()))?;
        Ok(raw_features.into_iter().collect())
    }

    async fn save_features(&self, features: Vec<(String, ClientFeatures)>) -> EdgeResult<()> {
        let mut client = self.redis_client.write().await;
        let raw_features = serde_json::to_string(&features)?;
        match &mut *client {
            Single(client) => client
                .set(FEATURES_KEY, raw_features)
                .map_err(EdgeError::from),
            Cluster(cluster) => {
                let mut conn = cluster.get_connection()?;
                conn.set(FEATURES_KEY, raw_features)
                    .map_err(EdgeError::from)
            }
        }
    }
}
