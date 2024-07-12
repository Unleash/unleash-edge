use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use redis::cluster::ClusterClient;
use redis::{AsyncCommands, Client, Commands, RedisError};
use tokio::sync::RwLock;
use tracing::{debug, info};
use unleash_types::client_features::ClientFeatures;

use crate::persistence::redis::RedisClientOptions::{Cluster, Single};
use crate::types::EdgeToken;
use crate::{error::EdgeError, types::EdgeResult};

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
    read_timeout: Duration,
    write_timeout: Duration,
    redis_client: Arc<RwLock<RedisClientOptions>>,
}
impl RedisPersister {
    pub fn new(
        url: &str,
        read_timeout: Duration,
        write_timeout: Duration,
    ) -> Result<RedisPersister, EdgeError> {
        let client = Client::open(url)?;
        let addr = client.get_connection_info().addr.clone();
        info!("[REDIS Persister]: Configured single node client {addr:?}");
        Ok(Self {
            redis_client: Arc::new(RwLock::new(Single(client))),
            read_timeout,
            write_timeout,
        })
    }
    pub fn new_with_cluster(
        urls: Vec<String>,
        read_timeout: Duration,
        write_timeout: Duration,
    ) -> Result<RedisPersister, EdgeError> {
        info!("[REDIS Persister]: Configuring cluster client against {urls:?}");
        let client = ClusterClient::builder(urls)
            .connection_timeout(read_timeout)
            .build()?;
        Ok(Self {
            redis_client: Arc::new(RwLock::new(Cluster(client))),
            read_timeout,
            write_timeout,
        })
    }
}

#[async_trait]
impl EdgePersistence for RedisPersister {
    async fn load_tokens(&self) -> EdgeResult<Vec<EdgeToken>> {
        debug!("Loading tokens from persistence");
        let mut client = self.redis_client.write().await;
        let raw_tokens: String = match &mut *client {
            Single(c) => {
                let mut conn = c
                    .get_multiplexed_tokio_connection_with_response_timeouts(
                        self.read_timeout,
                        self.read_timeout,
                    )
                    .await?;
                conn.get(TOKENS_KEY).await?
            }
            Cluster(c) => {
                let mut conn = c.get_connection()?;
                conn.get(TOKENS_KEY)?
            }
        };
        serde_json::from_str::<Vec<EdgeToken>>(&raw_tokens)
            .map_err(|_e| EdgeError::TokenParseError("Failed to load tokens from redis".into()))
    }

    async fn save_tokens(&self, tokens: Vec<EdgeToken>) -> EdgeResult<()> {
        debug!("Saving {} tokens to persistence", tokens.len());
        let mut client = self.redis_client.write().await;
        let raw_tokens = serde_json::to_string(&tokens)?;
        match &mut *client {
            RedisClientOptions::Single(c) => {
                let mut conn = c
                    .get_multiplexed_tokio_connection_with_response_timeouts(
                        self.write_timeout,
                        self.write_timeout,
                    )
                    .await?;
                conn.set(TOKENS_KEY, raw_tokens).await?;
            }
            RedisClientOptions::Cluster(c) => {
                let mut conn = c.get_connection()?;
                conn.set(TOKENS_KEY, raw_tokens)?
            }
        };
        Ok(())
    }

    async fn load_features(&self) -> EdgeResult<HashMap<String, ClientFeatures>> {
        debug!("Loading features from persistence");
        let mut client = self.redis_client.write().await;
        let raw_features: String = match &mut *client {
            Single(client) => {
                let mut conn = client
                    .get_multiplexed_tokio_connection_with_response_timeouts(
                        self.read_timeout,
                        self.read_timeout,
                    )
                    .await?;
                conn.get(FEATURES_KEY).await?
            }
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
        debug!("Saving {} features to persistence", features.len());
        let mut client = self.redis_client.write().await;
        let raw_features = serde_json::to_string(&features)?;
        match &mut *client {
            Single(client) => {
                let mut conn = client
                    .get_multiplexed_tokio_connection_with_response_timeouts(
                        self.write_timeout,
                        self.write_timeout,
                    )
                    .await?;
                conn.set(FEATURES_KEY, raw_features)
                    .await
                    .map_err(EdgeError::from)?
            }
            Cluster(cluster) => {
                let mut conn = cluster.get_connection()?;
                conn.set(FEATURES_KEY, raw_features)
                    .map_err(EdgeError::from)?
            }
        };
        debug!("Done saving to persistence");
        Ok(())
    }
}
