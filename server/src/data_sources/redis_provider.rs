use async_trait::async_trait;
use redis::AsyncCommands;
use redis::{Client, Commands, RedisError};
use tokio::sync::{mpsc::Sender, RwLock};
use unleash_types::client_features::{ClientFeature, ClientFeatures};
use unleash_types::Merge;

pub const FEATURE_PREFIX: &str = "unleash-feature-namespace:";
pub const TOKENS_KEY: &str = "unleash-token-namespace:";

use crate::types::TokenValidationStatus;
use crate::{
    error::EdgeError,
    types::{
        EdgeResult, EdgeSink, EdgeSource, EdgeToken, FeatureSink, FeaturesSource, TokenSink,
        TokenSource,
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

fn build_features_key(token: &EdgeToken) -> String {
    token
        .environment
        .as_ref()
        .map(|environment| format!("{FEATURE_PREFIX}{environment}"))
        .expect("Tying to resolve features for a token that hasn't been validated")
}

#[async_trait]
impl FeatureSink for RedisProvider {
    async fn sink_features(
        &mut self,
        token: &EdgeToken,
        features: ClientFeatures,
    ) -> EdgeResult<()> {
        let mut lock = self.redis_client.write().await;
        let mut con = lock.get_async_connection().await?;

        let key = build_features_key(token);

        let features_to_store =
            if let Some(stored_features) = con.get::<&str, Option<String>>(key.as_str()).await? {
                let stored_features = serde_json::from_str::<ClientFeatures>(&stored_features)?;
                stored_features.merge(features.clone())
            } else {
                features
            };
        let serialized_features = serde_json::to_string(&features_to_store)?;
        let _: () = lock.set(key, serialized_features)?;
        Ok(())
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
        let client_features: String = client.get(build_features_key(token))?;

        let features =
            serde_json::from_str::<ClientFeatures>(&client_features).map_err(EdgeError::from);

        features.map(|features| ClientFeatures {
            features: features
                .features
                .iter()
                .filter(|feature| {
                    if let Some(feature_project) = &feature.project {
                        token.projects.contains(&"*".to_string())
                            || token.projects.contains(feature_project)
                    } else {
                        false
                    }
                })
                .cloned()
                .collect::<Vec<ClientFeature>>(),
            ..features.clone()
        })
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

    async fn filter_valid_tokens(&self, _secrets: Vec<String>) -> EdgeResult<Vec<EdgeToken>> {
        todo!()
    }

    async fn token_details(&self, secret: String) -> EdgeResult<Option<EdgeToken>> {
        let tokens = self.get_known_tokens().await?;
        Ok(tokens.into_iter().find(|t| t.token == secret))
    }
}
