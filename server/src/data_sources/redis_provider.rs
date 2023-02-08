use async_trait::async_trait;
use redis::{Client, Commands, RedisError};
use tokio::sync::{mpsc::Sender, RwLock};
use unleash_types::client_features::ClientFeatures;

pub const FEATURE_PREFIX: &str = "unleash-edge-feature-";
pub const TOKENS_KEY: &str = "unleash-edge-tokens";

use crate::types::TokenValidationStatus;
use crate::{
    error::EdgeError,
    types::{
        ClientFeaturesResponse, EdgeResult, EdgeSink, EdgeSource, EdgeToken, FeatureSink,
        FeaturesSource, TokenSink, TokenSource,
    },
};

pub struct RedisProvider {
    redis_client: RwLock<Client>,
    sender: Sender<EdgeToken>,
}

impl From<RedisError> for EdgeError {
    fn from(err: RedisError) -> Self {
        EdgeError::DataSourceError(format!("Error connecting to Redis: {err}"))
    }
}

impl RedisProvider {
    pub fn new(url: &str, sender: Sender<EdgeToken>) -> Result<RedisProvider, EdgeError> {
        let client = redis::Client::open(url)?;
        Ok(Self {
            sender,
            redis_client: RwLock::new(client),
        })
    }
}
impl EdgeSource for RedisProvider {}
impl EdgeSink for RedisProvider {}

fn build_features_key(token: &String) -> String {
    format!("{FEATURE_PREFIX}{token}")
}

#[async_trait]
impl FeatureSink for RedisProvider {
    async fn sink_features(
        &mut self,
        token: &EdgeToken,
        features: ClientFeatures,
    ) -> EdgeResult<()> {
        let mut lock = self.redis_client.write().await;
        let serialized_features = serde_json::to_string(&features)?;
        let _: () = lock.set(build_features_key(&token.token), serialized_features)?;
        Ok(())
    }

    async fn fetch_features(&mut self, _token: &EdgeToken) -> EdgeResult<ClientFeaturesResponse> {
        todo!()
    }
}
#[async_trait]
impl TokenSink for RedisProvider {
    async fn sink_tokens(&mut self, _tokens: Vec<EdgeToken>) -> EdgeResult<()> {
        // let mut lock = self.redis_client.write().await;
        // let tokens: String = lock.get(TOKENS_KEY)?;
        // let tokens = serde_json::from_str::<Vec<EdgeToken>>(&tokens)?;
        Ok(())
    }
}

#[async_trait]
impl FeaturesSource for RedisProvider {
    async fn get_client_features(&self, token: &EdgeToken) -> EdgeResult<ClientFeatures> {
        let mut client = self.redis_client.write().await;
        let client_features: String = client.get(build_features_key(&token.token))?;

        serde_json::from_str::<ClientFeatures>(&client_features).map_err(EdgeError::from)
    }
}

#[async_trait]
impl TokenSource for RedisProvider {
    async fn get_known_tokens(&self) -> EdgeResult<Vec<EdgeToken>> {
        let mut client = self.redis_client.write().await;

        let tokens: String = client.get(TOKENS_KEY)?;

        let raw_tokens = serde_json::from_str::<Vec<String>>(&tokens)?;

        Ok(raw_tokens
            .into_iter()
            .map(EdgeToken::try_from)
            .filter_map(|t| t.ok())
            .collect())
    }

    async fn get_valid_tokens(&self) -> EdgeResult<Vec<EdgeToken>> {
        let tokens = self.get_known_tokens().await?;
        Ok(tokens
            .into_iter()
            .filter(|t| t.status == TokenValidationStatus::Validated)
            .collect())
    }

    async fn get_token_validation_status(&self, secret: &str) -> EdgeResult<TokenValidationStatus> {
        if let Some(t) = self
            .get_known_tokens()
            .await?
            .iter()
            .find(|t| t.token == secret)
        {
            Ok(t.clone().status)
        } else {
            let _ = self
                .sender
                .send(EdgeToken::try_from(secret.to_string())?)
                .await;
            Ok(TokenValidationStatus::Unknown)
        }
    }

    async fn filter_valid_tokens(&self, _secrets: Vec<String>) -> EdgeResult<Vec<EdgeToken>> {
        todo!()
    }

    async fn token_details(&self, secret: String) -> EdgeResult<Option<EdgeToken>> {
        let tokens = self.get_known_tokens().await?;
        Ok(tokens.into_iter().find(|t| t.token == secret))
    }
}
