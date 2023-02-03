use async_trait::async_trait;
use redis::{Client, Commands, RedisError};
use std::sync::Arc;
use tokio::sync::{mpsc::Sender, RwLock};
use unleash_types::client_features::ClientFeatures;

pub const FEATURE_KEY: &str = "features";
pub const TOKENS_KEY: &str = "tokens";

use crate::{
    error::EdgeError,
    types::{
        ClientFeaturesResponse, EdgeProvider, EdgeResult, EdgeSink, EdgeSource, EdgeToken,
        FeatureSink, FeaturesSource, TokenSink, TokenSource,
    },
};

pub struct RedisProvider {
    client: RwLock<Client>,
}

impl From<RedisError> for EdgeError {
    fn from(err: RedisError) -> Self {
        EdgeError::DataSourceError(format!("Error connecting to Redis: {err}"))
    }
}

impl RedisProvider {
    pub fn new(url: &str) -> Result<RedisProvider, EdgeError> {
        let client = redis::Client::open(url)?;
        Ok(Self {
            client: RwLock::new(client),
        })
    }
}

impl EdgeProvider for RedisProvider {}

impl EdgeSource for RedisProvider {}
impl EdgeSink for RedisProvider {}

#[async_trait]
impl FeatureSink for RedisProvider {
    async fn sink_features(
        &mut self,
        _token: &EdgeToken,
        _features: ClientFeatures,
    ) -> EdgeResult<()> {
        todo!()
    }
    async fn fetch_features(&mut self, _token: &EdgeToken) -> EdgeResult<ClientFeaturesResponse> {
        todo!()
    }
}
#[async_trait]
impl TokenSink for RedisProvider {
    async fn sink_tokens(&mut self, _tokens: Vec<EdgeToken>) -> EdgeResult<()> {
        todo!()
    }

    async fn validate(&mut self, _tokens: Vec<EdgeToken>) -> EdgeResult<Vec<EdgeToken>> {
        todo!()
    }
}

#[async_trait]
impl FeaturesSource for RedisProvider {
    async fn get_client_features(&self, _token: &EdgeToken) -> EdgeResult<ClientFeatures> {
        let mut client = self.client.write().await;
        let client_features: String = client.get(FEATURE_KEY)?;

        serde_json::from_str::<ClientFeatures>(&client_features).map_err(EdgeError::from)
    }
}

#[async_trait]
impl TokenSource for RedisProvider {
    async fn get_known_tokens(&self) -> EdgeResult<Vec<EdgeToken>> {
        let mut client = self.client.write().await;

        let tokens: String = client.get(TOKENS_KEY)?;

        let raw_tokens = serde_json::from_str::<Vec<String>>(&tokens)?;

        Ok(raw_tokens
            .into_iter()
            .map(EdgeToken::try_from)
            .filter_map(|t| t.ok())
            .collect())
    }

    async fn secret_is_valid(
        &self,
        secret: &str,
        sender: Arc<Sender<EdgeToken>>,
    ) -> EdgeResult<bool> {
        if self
            .get_known_tokens()
            .await?
            .iter()
            .any(|t| t.token == secret)
        {
            Ok(true)
        } else {
            let _ = sender.send(EdgeToken::try_from(secret.to_string())?).await;
            Ok(false)
        }
    }

    async fn get_valid_tokens(&self, _secrets: Vec<String>) -> EdgeResult<Vec<EdgeToken>> {
        todo!()
    }

    async fn token_details(&self, secret: String) -> EdgeResult<Option<EdgeToken>> {
        let tokens = self.get_known_tokens().await?;
        Ok(tokens.into_iter().find(|t| t.token == secret))
    }
}
