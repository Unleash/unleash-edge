use std::collections::HashMap;

use crate::types::TokenRefresh;
use crate::types::{EdgeResult, EdgeToken};
use actix_web::http::header::EntityTag;
use async_trait::async_trait;
use dashmap::DashMap;
use tracing::instrument;
use unleash_types::client_features::ClientFeatures;
use unleash_types::Merge;

use super::repository::{DataSink, DataSource};

#[derive(Debug, Clone)]
pub struct MemoryProvider {
    data_store: DashMap<String, ClientFeatures>,
    token_store: HashMap<String, EdgeToken>,
    tokens_to_refresh: HashMap<String, TokenRefresh>,
}

fn key(token: &EdgeToken) -> String {
    token.environment.clone().unwrap()
}

impl Default for MemoryProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryProvider {
    pub fn new() -> Self {
        Self {
            data_store: DashMap::new(),
            token_store: HashMap::new(),
            tokens_to_refresh: HashMap::new(),
        }
    }
}

#[async_trait]
impl DataSource for MemoryProvider {
    #[instrument(skip(self))]
    async fn get_tokens(&self) -> EdgeResult<Vec<EdgeToken>> {
        Ok(self.token_store.values().into_iter().cloned().collect())
    }

    #[instrument(skip(self, secret))]
    async fn get_token(&self, secret: &str) -> EdgeResult<Option<EdgeToken>> {
        Ok(self.token_store.get(secret).cloned())
    }

    #[instrument(skip(self))]
    async fn get_refresh_tokens(&self) -> EdgeResult<Vec<TokenRefresh>> {
        Ok(self.tokens_to_refresh.values().cloned().collect())
    }

    #[instrument(skip(self, token))]
    async fn get_client_features(&self, token: &EdgeToken) -> EdgeResult<Option<ClientFeatures>> {
        Ok(self.data_store.get(&key(token)).map(|v| v.value().clone()))
    }
}

#[async_trait]
impl DataSink for MemoryProvider {
    #[instrument(skip(self, tokens))]
    async fn sink_tokens(&mut self, tokens: Vec<EdgeToken>) -> EdgeResult<()> {
        for token in tokens {
            self.token_store.insert(token.token.clone(), token.clone());
        }
        Ok(())
    }

    #[instrument(skip(self, tokens))]
    async fn set_refresh_tokens(&mut self, tokens: Vec<&TokenRefresh>) -> EdgeResult<()> {
        let new_tokens = tokens
            .into_iter()
            .map(|token| (token.token.token.clone(), token.clone()))
            .collect();
        self.tokens_to_refresh = new_tokens;
        Ok(())
    }

    #[instrument(skip(self, token, features))]
    async fn sink_features(
        &mut self,
        token: &EdgeToken,
        features: ClientFeatures,
    ) -> EdgeResult<()> {
        self.data_store
            .entry(key(token))
            .and_modify(|data| {
                *data = data.clone().merge(features.clone());
            })
            .or_insert(features);
        Ok(())
    }

    #[instrument(skip(self, token))]
    async fn update_last_check(&mut self, token: &EdgeToken) -> EdgeResult<()> {
        if let Some(token) = self.tokens_to_refresh.get_mut(&token.token) {
            token.last_check = Some(chrono::Utc::now());
        }
        Ok(())
    }

    #[instrument(skip(self, token))]
    async fn update_last_refresh(
        &mut self,
        token: &EdgeToken,
        etag: Option<EntityTag>,
    ) -> EdgeResult<()> {
        if let Some(token) = self.tokens_to_refresh.get_mut(&token.token) {
            token.last_check = Some(chrono::Utc::now());
            token.last_refreshed = Some(chrono::Utc::now());
            token.etag = etag;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::types::TokenValidationStatus;

    use super::*;

    #[tokio::test]
    async fn memory_provider_correctly_deduplicates_tokens() {
        let mut provider = MemoryProvider::default();
        provider
            .sink_tokens(vec![EdgeToken {
                token: "*:development.1d38eefdd7bf72676122b008dcf330f2f2aa2f3031438e1b7e8f0d1f"
                    .into(),
                ..EdgeToken::default()
            }])
            .await
            .unwrap();

        provider
            .sink_tokens(vec![EdgeToken {
                token: "*:development.1d38eefdd7bf72676122b008dcf330f2f2aa2f3031438e1b7e8f0d1f"
                    .into(),
                ..EdgeToken::default()
            }])
            .await
            .unwrap();

        assert!(provider.get_tokens().await.unwrap().len() == 1);
    }

    #[tokio::test]
    async fn memory_provider_correctly_determines_token_to_be_valid() {
        let mut provider = MemoryProvider::default();
        provider
            .sink_tokens(vec![EdgeToken {
                token: "*:development.1d38eefdd7bf72676122b008dcf330f2f2aa2f3031438e1b7e8f0d1f"
                    .into(),
                status: TokenValidationStatus::Validated,
                ..EdgeToken::default()
            }])
            .await
            .unwrap();

        assert_eq!(
            provider
                .get_token("*:development.1d38eefdd7bf72676122b008dcf330f2f2aa2f3031438e1b7e8f0d1f")
                .await
                .expect("Could not retrieve token details")
                .unwrap()
                .status,
            TokenValidationStatus::Validated
        )
    }
}
