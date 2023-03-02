use async_trait::async_trait;
use unleash_types::client_features::ClientFeatures;

use crate::types::TokenRefresh;

pub mod redis;

#[async_trait]
pub trait EdgePersistence {
    async fn load_tokens(&self) -> Vec<TokenRefresh>;
    async fn save_tokens(&self, tokens: Vec<TokenRefresh>);
    async fn load_features(&self) -> Vec<ClientFeatures>;
    async fn save_features(&self, features: Vec<ClientFeatures>);
}