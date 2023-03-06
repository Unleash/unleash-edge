use std::collections::HashMap;

use async_trait::async_trait;
use unleash_types::client_features::ClientFeatures;

use crate::types::{EdgeResult, EdgeToken, TokenRefresh};

pub mod file;
pub mod redis;

#[async_trait]
pub trait EdgePersistence: Send + Sync {
    async fn load_tokens(&self) -> EdgeResult<Vec<EdgeToken>>;
    async fn save_tokens(&self, tokens: Vec<EdgeToken>) -> EdgeResult<()>;
    async fn load_refresh_targets(&self) -> EdgeResult<Vec<TokenRefresh>>;
    async fn save_refresh_targets(&self, refresh_targets: Vec<TokenRefresh>) -> EdgeResult<()>;
    async fn load_features(&self) -> EdgeResult<HashMap<String, ClientFeatures>>;
    async fn save_features(&self, features: Vec<(String, ClientFeatures)>) -> EdgeResult<()>;
}
