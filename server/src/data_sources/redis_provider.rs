use actix_web::http::header::EntityTag;
use async_trait::async_trait;
use chrono::{Duration, Utc};
use redis::{Client, Commands, RedisError};
use tokio::sync::RwLock;
use unleash_types::client_features::ClientFeatures;
use unleash_types::Merge;

pub const FEATURE_PREFIX: &str = "unleash-features";
pub const TOKENS_KEY: &str = "unleash-tokens";
pub const REFRESH_TOKENS_KEY: &str = "unleash-refresh-tokens";

use crate::types::FeatureRefresh;
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
    redis_client: RwLock<Client>,
    features_refresh_interval: Duration,
}

fn key(token: &EdgeToken) -> String {
    token
        .environment
        .as_ref()
        .map(|environment| format!("{FEATURE_PREFIX}:{environment}"))
        .expect("Trying to resolve features for a token that hasn't been validated")
}

impl RedisProvider {
    pub fn new(
        url: &str,
        features_refresh_interval_seconds: i64,
    ) -> Result<RedisProvider, EdgeError> {
        let client = redis::Client::open(url)?;

        Ok(Self {
            redis_client: RwLock::new(client),
            features_refresh_interval: Duration::seconds(features_refresh_interval_seconds),
        })
    }
}

#[async_trait]
impl DataSource for RedisProvider {
    async fn get_tokens(&self) -> EdgeResult<Vec<EdgeToken>> {
        let client = self.redis_client.read().await;
        let raw_tokens: String = client.get(TOKENS_KEY)?;
        let tokens = serde_json::from_str::<Vec<EdgeToken>>(&raw_tokens)?;

        Ok(tokens)
    }

    async fn get_token(&self, secret: &str) -> EdgeResult<Option<EdgeToken>> {
        let tokens = self.get_tokens().await?;
        Ok(tokens.into_iter().find(|t| t.token == secret))
    }

    async fn get_tokens_due_for_refresh(&self) -> EdgeResult<Vec<FeatureRefresh>> {
        let client = self.redis_client.read().await;
        let raw_refresh_tokens: String = client.get(REFRESH_TOKENS_KEY)?;
        let refresh_tokens = serde_json::from_str::<Vec<FeatureRefresh>>(&raw_refresh_tokens)?;

        Ok(refresh_tokens
            .into_iter()
            .filter(|token| {
                token
                    .last_check
                    .map(|last| Utc::now() - last > self.features_refresh_interval)
                    .unwrap_or(true)
            })
            .collect())
    }

    async fn get_client_features(&self, token: &EdgeToken) -> EdgeResult<Option<ClientFeatures>> {
        let client = self.redis_client.read().await;
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
        let raw_tokens: String = client.get(TOKENS_KEY)?;
        let raw_refresh_tokens: String = client.get(REFRESH_TOKENS_KEY)?;
        let tokens = serde_json::from_str::<Vec<EdgeToken>>(&raw_tokens).unwrap_or(vec![]);
        let refresh_tokens =
            serde_json::from_str::<Vec<FeatureRefresh>>(&raw_refresh_tokens).unwrap_or(vec![]);

        for token in tokens {
            tokens.push(token);
            if token.token_type == Some(crate::types::TokenType::Client)
                && !refresh_tokens.iter().any(|t| t.token.token == token.token)
            {
                refresh_tokens.push(FeatureRefresh::new(token.clone()));
            }
        }

        let refresh_tokens_tokens: Vec<EdgeToken> = refresh_tokens
            .into_iter()
            .map(|r| r.token.clone())
            .collect();
        let minimized_tokens = crate::tokens::simplify(&refresh_tokens_tokens);
        refresh_tokens.retain(|refresh_token| {
            minimized_tokens
                .iter()
                .any(|minimized_token| minimized_token.token == refresh_token.token.token)
        });

        let serialized_tokens = serde_json::to_string(&tokens)?;
        let serialized_refresh_tokens = serde_json::to_string(&refresh_tokens)?;

        client.set(TOKENS_KEY, serialized_tokens)?;
        client.set(REFRESH_TOKENS_KEY, serialized_refresh_tokens)?;

        Ok(())
    }

    async fn sink_features(
        &self,
        token: &EdgeToken,
        features: ClientFeatures,
        etag: Option<EntityTag>,
    ) -> EdgeResult<()> {
        let mut client = self.redis_client.write().await;
        let raw_refresh_tokens: String = client.get(REFRESH_TOKENS_KEY)?;
        let refresh_tokens =
            serde_json::from_str::<Vec<FeatureRefresh>>(&raw_refresh_tokens).unwrap_or(vec![]);
        let raw_stored_features: String = client.get(key(token))?;
        let stored_features = serde_json::from_str::<ClientFeatures>(&raw_stored_features).ok();

        if let mut feature_refresh = refresh_tokens
            .into_iter()
            .find(|t| t.token.token == token.token)
        {
            feature_refresh.unwrap().etag = etag.clone();
            feature_refresh.unwrap().last_refreshed = Some(Utc::now());
            feature_refresh.unwrap().last_check = Some(Utc::now());
        } else {
            refresh_tokens.push(FeatureRefresh {
                token: token.clone(),
                etag,
                last_refreshed: Some(Utc::now()),
                last_check: Some(Utc::now()),
            });
        }

        let features_to_store = match stored_features {
            Some(f) => f.merge(features),
            None => features,
        };

        let serialized_refresh_tokens = serde_json::to_string(&refresh_tokens)?;
        let serialized_features_to_store = serde_json::to_string(&features_to_store)?;

        client.set(REFRESH_TOKENS_KEY, serialized_refresh_tokens)?;
        client.set(key(token), serialized_features_to_store)?;

        Ok(())
    }
}
