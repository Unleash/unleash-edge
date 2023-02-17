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

use super::repository::{DataSink, DataSource};

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
    async fn sink_tokens(&mut self, tokens: Vec<EdgeToken>) -> EdgeResult<()> {
        let mut client = self.redis_client.write().await;
        let raw_stored_tokens: String = client.get(TOKENS_KEY)?;
        let mut stored_tokens = serde_json::from_str::<Vec<EdgeToken>>(&raw_stored_tokens).unwrap_or(vec![]);

        for token in tokens {
            stored_tokens.push(token);
        }

        let serialized_tokens = serde_json::to_string(&stored_tokens)?;
        client.set(TOKENS_KEY, serialized_tokens)?;

        Ok(())
    }

    async fn sink_refresh_tokens(&mut self, tokens: Vec<&TokenRefresh>) -> EdgeResult<()> {
        let mut client = self.redis_client.write().await;
        let raw_refresh_tokens: String = client.get(REFRESH_TOKENS_KEY)?;
        let mut refresh_tokens =
            serde_json::from_str::<Vec<TokenRefresh>>(&raw_refresh_tokens).unwrap_or(vec![]);

        for token in tokens {
            if !refresh_tokens.iter().any(|t| t.token.token == token.token.token) {
                refresh_tokens.push(token.clone());
            }
        }

        Ok(())
    }
    async fn sink_features(
        &mut self,
        token: &EdgeToken,
        features: ClientFeatures,
    ) -> EdgeResult<()> {
        let mut client = self.redis_client.write().await;
        let raw_stored_features: String = client.get(key(token))?;
        let stored_features = serde_json::from_str::<ClientFeatures>(&raw_stored_features).ok();

        let features_to_store = match stored_features {
            Some(f) => f.merge(features),
            None => features,
        };

        let serialized_features_to_store = serde_json::to_string(&features_to_store)?;
        client.set(key(token), serialized_features_to_store)?;

        Ok(())
    }

    async fn update_last_check(&mut self, token: &EdgeToken) -> EdgeResult<()>{
        let mut client = self.redis_client.write().await;
        let raw_refresh_tokens: String = client.get(REFRESH_TOKENS_KEY)?;
        let mut refresh_tokens =
            serde_json::from_str::<Vec<TokenRefresh>>(&raw_refresh_tokens).unwrap_or(vec![]);

        if let Some(token) = refresh_tokens.iter_mut().find(|t| t.token.token == token.token) {
            token.last_check = Some(chrono::Utc::now());
        }

        Ok(())
    }

    async fn update_last_refresh(&mut self, token: &EdgeToken, etag: Option<EntityTag>) -> EdgeResult<()> {
        let mut client = self.redis_client.write().await;
        let raw_refresh_tokens: String = client.get(REFRESH_TOKENS_KEY)?;
        let mut refresh_tokens =
            serde_json::from_str::<Vec<TokenRefresh>>(&raw_refresh_tokens).unwrap_or(vec![]);

        if let Some(token) = refresh_tokens.iter_mut().find(|t| t.token.token == token.token) {
            token.last_check = Some(chrono::Utc::now());
            token.last_refreshed = Some(chrono::Utc::now());
            token.etag = etag;
        }

        Ok(())
    }

    // async fn sink_tokens(&self, tokens: Vec<EdgeToken>) -> EdgeResult<()> {
    //     let mut client = self.redis_client.write().await;
    //     let raw_tokens: String = client.get(TOKENS_KEY)?;
    //     let raw_refresh_tokens: String = client.get(REFRESH_TOKENS_KEY)?;
    //     let tokens = serde_json::from_str::<Vec<EdgeToken>>(&raw_tokens).unwrap_or(vec![]);
    //     let refresh_tokens =
    //         serde_json::from_str::<Vec<FeatureRefresh>>(&raw_refresh_tokens).unwrap_or(vec![]);

    //     for token in tokens {
    //         tokens.push(token);
    //         if token.token_type == Some(crate::types::TokenType::Client)
    //             && !refresh_tokens.iter().any(|t| t.token.token == token.token)
    //         {
    //             refresh_tokens.push(FeatureRefresh::new(token.clone()));
    //         }
    //     }

    //     let refresh_tokens_tokens: Vec<EdgeToken> = refresh_tokens
    //         .into_iter()
    //         .map(|r| r.token.clone())
    //         .collect();
    //     let minimized_tokens = crate::tokens::simplify(&refresh_tokens_tokens);
    //     refresh_tokens.retain(|refresh_token| {
    //         minimized_tokens
    //             .iter()
    //             .any(|minimized_token| minimized_token.token == refresh_token.token.token)
    //     });

    //     let serialized_tokens = serde_json::to_string(&tokens)?;
    //     let serialized_refresh_tokens = serde_json::to_string(&refresh_tokens)?;

    //     client.set(TOKENS_KEY, serialized_tokens)?;
    //     client.set(REFRESH_TOKENS_KEY, serialized_refresh_tokens)?;

    //     Ok(())
    // }

    // async fn sink_features(
    //     &self,
    //     token: &EdgeToken,
    //     features: ClientFeatures,
    //     etag: Option<EntityTag>,
    // ) -> EdgeResult<()> {
    //     let mut client = self.redis_client.write().await;
    //     let raw_refresh_tokens: String = client.get(REFRESH_TOKENS_KEY)?;
    //     let refresh_tokens =
    //         serde_json::from_str::<Vec<FeatureRefresh>>(&raw_refresh_tokens).unwrap_or(vec![]);
    //     let raw_stored_features: String = client.get(key(token))?;
    //     let stored_features = serde_json::from_str::<ClientFeatures>(&raw_stored_features).ok();

    //     if let mut feature_refresh = refresh_tokens
    //         .into_iter()
    //         .find(|t| t.token.token == token.token)
    //     {
    //         feature_refresh.unwrap().etag = etag.clone();
    //         feature_refresh.unwrap().last_refreshed = Some(Utc::now());
    //         feature_refresh.unwrap().last_check = Some(Utc::now());
    //     } else {
    //         refresh_tokens.push(FeatureRefresh {
    //             token: token.clone(),
    //             etag,
    //             last_refreshed: Some(Utc::now()),
    //             last_check: Some(Utc::now()),
    //         });
    //     }

    //     let features_to_store = match stored_features {
    //         Some(f) => f.merge(features),
    //         None => features,
    //     };

    //     let serialized_refresh_tokens = serde_json::to_string(&refresh_tokens)?;
    //     let serialized_features_to_store = serde_json::to_string(&features_to_store)?;

    //     client.set(REFRESH_TOKENS_KEY, serialized_refresh_tokens)?;
    //     client.set(key(token), serialized_features_to_store)?;

    //     Ok(())
}
