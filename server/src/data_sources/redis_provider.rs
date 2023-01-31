use std::{sync::RwLock};

use redis::{Client, Commands, RedisError};
use unleash_types::client_features::ClientFeatures;

pub const FEATURE_KEY: &str = "features";
pub const TOKENS_KEY: &str = "tokens";

use crate::{
    error::EdgeError,
    types::{EdgeResult, EdgeToken, FeaturesProvider, TokenProvider, EdgeProvider},
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

impl FeaturesProvider for RedisProvider {
    fn get_client_features(&self, _token: EdgeToken) -> EdgeResult<ClientFeatures> {
        let mut client = self.client.write().unwrap();
        let client_features: String = client.get(FEATURE_KEY)?;
        serde_json::from_str::<ClientFeatures>(&client_features).map_err(EdgeError::from)
    }
}

impl TokenProvider for RedisProvider {
    fn get_known_tokens(&self) -> EdgeResult<Vec<EdgeToken>> {
        let mut client = self.client.write().unwrap();
        let tokens: String = client.get(TOKENS_KEY)?;
        serde_json::from_str::<Vec<EdgeToken>>(&tokens).map_err(EdgeError::from)
    }

    fn secret_is_valid(&self, secret: &str) -> EdgeResult<bool> {
        Ok(self.get_known_tokens()?.iter().any(|t| t.secret == secret))
    }

    fn token_details(&self, secret: String) -> EdgeResult<Option<EdgeToken>> {
        let tokens = self.get_known_tokens()?;
        Ok(tokens.into_iter().find(|t| t.secret == secret))
    }
}
