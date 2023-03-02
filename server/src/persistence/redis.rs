use std::sync::Arc;

use actix_web::http::header::EntityTag;
use async_trait::async_trait;
use redis::{Client, Commands, RedisError};
use tokio::sync::RwLock;
use unleash_types::client_features::ClientFeatures;
use unleash_types::Merge;

pub const FEATURE_PREFIX: &str = "unleash-features";
pub const TOKENS_KEY: &str = "unleash-tokens";
pub const REFRESH_TOKENS_KEY: &str = "unleash-refresh-tokens";

use crate::types::TokenRefresh;
use crate::{
    error::EdgeError,
    types::{EdgeResult, EdgeToken},
};


impl From<RedisError> for EdgeError {
    fn from(err: RedisError) -> Self {
        EdgeError::DataSourceError(format!("Error connecting to Redis: {err}"))
    }
}

pub struct RedisProvider {
    redis_client: Arc<RwLock<Client>>,
}

fn key(token: &EdgeToken) -> String {
    let environment = token.environment.clone().unwrap();
    format!("{FEATURE_PREFIX}:{environment}")
}

impl RedisProvider {
    pub fn new(url: &str) -> Result<RedisProvider, EdgeError> {
        let client = Arc::new(RwLock::new(redis::Client::open(url)?));

        Ok(Self {
            redis_client: client,
        })
    }
}

#[async_trait]
impl DataSource for RedisProvider {
    async fn get_tokens(&self) -> EdgeResult<Vec<EdgeToken>> {
        let mut client = self.redis_client.write().await;
        let raw_tokens: String = client.get(TOKENS_KEY)?;
        let tokens = serde_json::from_str::<Vec<EdgeToken>>(&raw_tokens)?;

        Ok(tokens)
    }

    async fn get_token(&self, secret: &str) -> EdgeResult<Option<EdgeToken>> {
        let tokens = self.get_tokens().await?;

        Ok(tokens.into_iter().find(|t| t.token == secret))
    }

    async fn get_refresh_tokens(&self) -> EdgeResult<Vec<TokenRefresh>> {
        let mut client = self.redis_client.write().await;
        let raw_refresh_tokens: String = client.get(REFRESH_TOKENS_KEY)?;
        let refresh_tokens = serde_json::from_str::<Vec<TokenRefresh>>(&raw_refresh_tokens)?;

        Ok(refresh_tokens)
    }

    async fn get_client_features(&self, token: &EdgeToken) -> EdgeResult<Option<ClientFeatures>> {
        let mut client = self.redis_client.write().await;
        let raw_features: String = client.get(key(token))?;
        let features =
            serde_json::from_str::<ClientFeatures>(&raw_features).map_err(EdgeError::from);

        Ok(features.ok())
    }
}

#[async_trait]
impl DataSink for RedisProvider {
    async fn sink_tokens(&self, tokens: Vec<EdgeToken>) -> EdgeResult<()> {
        let mut client = self.redis_client.write().await;
        let raw_stored_tokens: Option<String> = client.get(TOKENS_KEY)?;

        let mut stored_tokens = match raw_stored_tokens {
            Some(raw_stored_tokens) => serde_json::from_str::<Vec<EdgeToken>>(&raw_stored_tokens)?,
            None => vec![],
        };

        for token in tokens {
            stored_tokens.push(token);
        }

        let serialized_tokens = serde_json::to_string(&stored_tokens)?;
        client.set(TOKENS_KEY, serialized_tokens)?;

        Ok(())
    }

    async fn set_refresh_tokens(&self, tokens: Vec<&TokenRefresh>) -> EdgeResult<()> {
        let mut client = self.redis_client.write().await;

        let serialized_refresh_tokens = serde_json::to_string(&tokens)?;
        client.set(REFRESH_TOKENS_KEY, serialized_refresh_tokens)?;

        Ok(())
    }

    async fn sink_features(&self, token: &EdgeToken, features: ClientFeatures) -> EdgeResult<()> {
        let mut client = self.redis_client.write().await;
        let raw_stored_features: Option<String> = client.get(key(token))?;

        let features_to_store = match raw_stored_features {
            Some(raw_stored_features) => {
                let stored_features = serde_json::from_str::<ClientFeatures>(&raw_stored_features)?;
                stored_features.merge(features)
            }
            None => features,
        };

        let serialized_features_to_store = serde_json::to_string(&features_to_store)?;
        client.set(key(token), serialized_features_to_store)?;

        Ok(())
    }

    async fn update_last_check(&self, token: &EdgeToken) -> EdgeResult<()> {
        let mut client = self.redis_client.write().await;
        let raw_refresh_tokens: Option<String> = client.get(REFRESH_TOKENS_KEY)?;

        let mut refresh_tokens = match raw_refresh_tokens {
            Some(raw_refresh_tokens) => {
                serde_json::from_str::<Vec<TokenRefresh>>(&raw_refresh_tokens)?
            }
            None => vec![],
        };

        if let Some(token) = refresh_tokens
            .iter_mut()
            .find(|t| t.token.token == token.token)
        {
            token.last_check = Some(chrono::Utc::now());
        }

        let serialized_refresh_tokens = serde_json::to_string(&refresh_tokens)?;
        client.set(REFRESH_TOKENS_KEY, serialized_refresh_tokens)?;

        Ok(())
    }

    async fn update_last_refresh(
        &self,
        token: &EdgeToken,
        etag: Option<EntityTag>,
    ) -> EdgeResult<()> {
        let mut client = self.redis_client.write().await;
        let raw_refresh_tokens: Option<String> = client.get(REFRESH_TOKENS_KEY)?;

        let mut refresh_tokens = match raw_refresh_tokens {
            Some(raw_refresh_tokens) => {
                serde_json::from_str::<Vec<TokenRefresh>>(&raw_refresh_tokens)?
            }
            None => vec![],
        };

        if let Some(token) = refresh_tokens
            .iter_mut()
            .find(|t| t.token.token == token.token)
        {
            token.last_check = Some(chrono::Utc::now());
            token.last_refreshed = Some(chrono::Utc::now());
            token.etag = etag;
        }

        let serialized_refresh_tokens = serde_json::to_string(&refresh_tokens)?;
        client.set(REFRESH_TOKENS_KEY, serialized_refresh_tokens)?;

        Ok(())
    }
}
