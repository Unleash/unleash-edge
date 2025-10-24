use ahash::HashMap;
use std::sync::Arc;
use std::time::Duration;
use unleash_edge_types::enterprise::LicenseState;

use super::EdgePersistence;
use crate::redis::RedisClientOptions::{Cluster, Single};
use async_trait::async_trait;
use redis::cluster::ClusterClient;
use redis::{AsyncCommands, Client, Commands, RedisError};
use tokio::sync::RwLock;
use tracing::{debug, info};
use unleash_edge_types::EdgeResult;
use unleash_edge_types::errors::EdgeError;
use unleash_edge_types::tokens::EdgeToken;
use unleash_types::client_features::ClientFeatures;

pub const FEATURES_KEY: &str = "unleash-features";
pub const TOKENS_KEY: &str = "unleash-tokens";
pub const LICENSE_STATE_KEY: &str = "unleash-license-state";

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
        if let Err(err) = rustls::crypto::ring::default_provider().install_default() {
            info!(
                "Failed to install default crypto provider, this is likely because another system has already installed it: {:?}",
                err
            );
        }
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
        if let Err(err) = rustls::crypto::ring::default_provider().install_default() {
            info!(
                "Failed to install default crypto provider, this is likely because another system has already installed it: {:?}",
                err
            );
        }
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
                let res: Result<(), RedisError> = conn.set(TOKENS_KEY, raw_tokens).await;
                res?;
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
                let res: Result<(), EdgeError> = conn
                    .set(FEATURES_KEY, raw_features)
                    .map_err(EdgeError::from);
                res?;
            }
        };
        debug!("Done saving to persistence");
        Ok(())
    }

    async fn load_license_state(&self) -> LicenseState {
        debug!("Loading license state from persistence");
        let mut client = self.redis_client.write().await;
        let raw_license_state: String = match &mut *client {
            Single(c) => {
                let Ok(mut conn) = c
                    .get_multiplexed_tokio_connection_with_response_timeouts(
                        self.read_timeout,
                        self.read_timeout,
                    )
                    .await
                else {
                    return LicenseState::Undetermined;
                };
                let Ok(raw_license_state) = conn.get(LICENSE_STATE_KEY).await else {
                    return LicenseState::Undetermined;
                };
                raw_license_state
            }
            Cluster(c) => {
                let Ok(mut conn) = c.get_connection() else {
                    return LicenseState::Undetermined;
                };
                let Ok(raw_license_state) = conn.get(LICENSE_STATE_KEY) else {
                    return LicenseState::Undetermined;
                };
                raw_license_state
            }
        };
        serde_json::from_str::<LicenseState>(&raw_license_state)
            .unwrap_or(LicenseState::Undetermined)
    }

    async fn save_license_state(
        &self,
        license_state: &LicenseState,
    ) -> EdgeResult<()> {
        debug!("Saving license state to persistence");
        let mut client = self.redis_client.write().await;
        let raw_license_state = serde_json::to_string(&license_state)?;
        match &mut *client {
            RedisClientOptions::Single(c) => {
                let mut conn = c
                    .get_multiplexed_tokio_connection_with_response_timeouts(
                        self.write_timeout,
                        self.write_timeout,
                    )
                    .await?;
                let res: Result<(), RedisError> =
                    conn.set(LICENSE_STATE_KEY, raw_license_state).await;
                res?;
            }
            RedisClientOptions::Cluster(c) => {
                let mut conn = c.get_connection()?;
                conn.set(LICENSE_STATE_KEY, raw_license_state)?
            }
        };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use redis::Client;
    use std::{str::FromStr, time::Duration};
    use testcontainers_modules::redis::RedisStack;
    use unleash_edge_types::{TokenType, tokens::EdgeToken};
    use unleash_types::client_features::{ClientFeature, ClientFeatures};

    use testcontainers::{ContainerAsync, runners::AsyncRunner};

    const TEST_TIMEOUT: Duration = std::time::Duration::from_millis(1000);

    async fn setup_redis() -> (Client, String, ContainerAsync<RedisStack>) {
        let node = RedisStack::default()
            .start()
            .await
            .expect("Failed to start redis");
        let host_port = node
            .get_host_port_ipv4(6379)
            .await
            .expect("Could not get port");
        let url = format!("redis://127.0.0.1:{host_port}");

        (Client::open(url.clone()).unwrap(), url, node)
    }

    #[tokio::test]
    async fn redis_saves_and_restores_features_correctly() {
        let (_client, url, _node) = setup_redis().await;
        let redis_persister = RedisPersister::new(&url, TEST_TIMEOUT, TEST_TIMEOUT).unwrap();

        let features = ClientFeatures {
            features: vec![ClientFeature {
                name: "test".to_string(),
                ..ClientFeature::default()
            }],
            query: None,
            segments: None,
            version: 2,
            meta: None,
        };
        let environment = "development";
        redis_persister
            .save_features(vec![(environment.into(), features.clone())])
            .await
            .unwrap();
        let results = redis_persister.load_features().await.unwrap();
        assert_eq!(results.get(environment).unwrap(), &features);
    }

    #[tokio::test]
    async fn redis_saves_and_restores_edge_tokens_correctly() {
        let (_client, url, _node) = setup_redis().await;
        let redis_persister = RedisPersister::new(&url, TEST_TIMEOUT, TEST_TIMEOUT).unwrap();
        let mut project_specific_token =
            EdgeToken::from_str("someproject:development.abcdefghijklmnopqr").unwrap();
        project_specific_token.token_type = Some(TokenType::Backend);
        let mut wildcard_token = EdgeToken::from_str("*:development.mysecretispersonal").unwrap();
        wildcard_token.token_type = Some(TokenType::Backend);
        redis_persister
            .save_tokens(vec![project_specific_token, wildcard_token])
            .await
            .unwrap();
        let saved_tokens = redis_persister.load_tokens().await.unwrap();
        assert_eq!(saved_tokens.len(), 2);
    }

    #[tokio::test]
    async fn redis_saves_and_restores_license_state_correctly() {
        let (_client, url, _node) = setup_redis().await;
        let redis_persister = RedisPersister::new(&url, TEST_TIMEOUT, TEST_TIMEOUT).unwrap();
        let license_state = LicenseState::Valid;
        redis_persister
            .save_license_state(&license_state)
            .await
            .unwrap();
        let loaded_state = redis_persister.load_license_state().await;
        assert_eq!(loaded_state, license_state);
    }

    #[tokio::test]
    async fn redis_returns_undetermined_license_state_when_no_state_saved() {
        let (_client, url, _node) = setup_redis().await;
        let redis_persister = RedisPersister::new(&url, TEST_TIMEOUT, TEST_TIMEOUT).unwrap();
        let loaded_state = redis_persister.load_license_state().await;
        assert_eq!(loaded_state, LicenseState::Undetermined);
    }

    #[tokio::test]
    async fn redis_returns_undetermined_license_state_when_an_error_occurs() {
        let (_client, url, mut _node) = setup_redis().await;
        let redis_persister = RedisPersister::new(&url, TEST_TIMEOUT, TEST_TIMEOUT).unwrap();
        // Stop the redis node to simulate an error
        _node.stop().await.expect("Failed to stop redis");
        let loaded_state = redis_persister.load_license_state().await;
        assert_eq!(loaded_state, LicenseState::Undetermined);
    }
}
