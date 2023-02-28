use crate::types::TokenRefresh;
use crate::types::{EdgeResult, EdgeToken};
use actix_web::http::header::EntityTag;
use async_trait::async_trait;
use dashmap::DashMap;
use unleash_types::client_features::ClientFeatures;

use super::repository::{DataSink, DataSource};

#[derive(Debug, Clone)]
pub struct MemoryProvider {
    data_store: DashMap<String, ClientFeatures>,
    token_store: DashMap<String, EdgeToken>,
    tokens_to_refresh: DashMap<String, TokenRefresh>,
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
            token_store: DashMap::new(),
            tokens_to_refresh: DashMap::new(),
        }
    }
}

#[async_trait]
impl DataSource for MemoryProvider {
    async fn get_tokens(&self) -> EdgeResult<Vec<EdgeToken>> {
        Ok(self.token_store.iter().map(|x| x.value().clone()).collect())
    }

    async fn get_token(&self, secret: &str) -> EdgeResult<Option<EdgeToken>> {
        Ok(self.token_store.get(secret).map(|x| x.clone()))
    }

    async fn get_refresh_tokens(&self) -> EdgeResult<Vec<TokenRefresh>> {
        Ok(self
            .tokens_to_refresh
            .iter()
            .map(|x| x.value().clone())
            .collect())
    }

    async fn get_client_features(&self, token: &EdgeToken) -> EdgeResult<Option<ClientFeatures>> {
        Ok(self.data_store.get(&key(token)).map(|v| v.value().clone()))
    }
}

#[async_trait]
impl DataSink for MemoryProvider {
    async fn sink_tokens(&self, tokens: Vec<EdgeToken>) -> EdgeResult<()> {
        for token in tokens {
            self.token_store.insert(token.token.clone(), token.clone());
        }
        Ok(())
    }

    async fn set_refresh_tokens(&self, tokens: Vec<&TokenRefresh>) -> EdgeResult<()> {
        self.tokens_to_refresh.clear();
        tokens.into_iter().for_each(|refresh| {
            self.tokens_to_refresh
                .insert(refresh.token.token.clone(), refresh.clone());
        });
        Ok(())
    }

    async fn sink_features(&self, token: &EdgeToken, features: ClientFeatures) -> EdgeResult<()> {
        self.data_store.insert(key(token), features);
        Ok(())
    }

    async fn update_last_check(&self, token: &EdgeToken) -> EdgeResult<()> {
        if let Some(mut token) = self.tokens_to_refresh.get_mut(&token.token) {
            token.last_check = Some(chrono::Utc::now());
        }
        Ok(())
    }

    async fn update_last_refresh(
        &self,
        token: &EdgeToken,
        etag: Option<EntityTag>,
    ) -> EdgeResult<()> {
        if let Some(mut token) = self.tokens_to_refresh.get_mut(&token.token) {
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
        let provider = MemoryProvider::default();
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
        let provider = MemoryProvider::default();
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
