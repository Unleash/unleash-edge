use std::collections::HashMap;

use crate::types::ClientFeaturesResponse;
use crate::types::TokenValidationStatus;
use crate::types::{
    EdgeResult, EdgeSink, EdgeSource, EdgeToken, FeatureSink, FeaturesSource, TokenSink,
    TokenSource,
};
use async_trait::async_trait;
use dashmap::DashMap;
use tokio::sync::mpsc::Sender;
use unleash_types::client_features::ClientFeatures;

#[derive(Debug, Clone)]
pub struct MemoryProvider {
    data_store: DashMap<String, ClientFeatures>,
    token_store: HashMap<String, EdgeToken>,
    sender: Sender<EdgeToken>,
}

impl MemoryProvider {
    pub fn new(sender: Sender<EdgeToken>) -> Self {
        Self {
            data_store: DashMap::new(),
            token_store: HashMap::new(),
            sender,
        }
    }

    fn sink_features(&mut self, token: &EdgeToken, features: ClientFeatures) {
        self.data_store.insert(token.token.clone(), features);
    }
}

impl EdgeSource for MemoryProvider {}
impl EdgeSink for MemoryProvider {}

#[async_trait]
impl TokenSink for MemoryProvider {
    async fn sink_tokens(&mut self, tokens: Vec<EdgeToken>) -> EdgeResult<()> {
        for token in &tokens {
            self.token_store.insert(token.token.clone(), token.clone());
        }
        Ok(())
    }
}

#[async_trait]
impl FeaturesSource for MemoryProvider {
    async fn get_client_features(&self, token: &EdgeToken) -> EdgeResult<ClientFeatures> {
        Ok(self
            .data_store
            .get(&token.token)
            .map(|v| v.value().clone())
            .unwrap_or_else(|| ClientFeatures {
                version: 2,
                features: vec![],
                segments: None,
                query: None,
            }))
    }
}

#[async_trait]
impl TokenSource for MemoryProvider {
    async fn get_known_tokens(&self) -> EdgeResult<Vec<EdgeToken>> {
        Ok(self.token_store.values().into_iter().cloned().collect())
    }

    async fn get_valid_tokens(&self) -> EdgeResult<Vec<EdgeToken>> {
        Ok(self
            .token_store
            .values()
            .filter(|t| t.status == TokenValidationStatus::Validated)
            .cloned()
            .collect())
    }

    async fn get_token_validation_status(&self, secret: &str) -> EdgeResult<TokenValidationStatus> {
        if let Some(token) = self.token_store.get(secret) {
            Ok(token.clone().status)
        } else {
            let _ = self
                .sender
                .send(EdgeToken::try_from(secret.to_string())?)
                .await;
            Ok(TokenValidationStatus::Unknown)
        }
    }

    async fn token_details(&self, secret: String) -> EdgeResult<Option<EdgeToken>> {
        Ok(self.token_store.get(&secret).cloned())
    }

    async fn filter_valid_tokens(&self, secrets: Vec<String>) -> EdgeResult<Vec<EdgeToken>> {
        Ok(secrets
            .iter()
            .filter_map(|s| self.token_store.get(s))
            .filter(|s| s.status == TokenValidationStatus::Validated)
            .cloned()
            .collect())
    }
}

#[async_trait]
impl FeatureSink for MemoryProvider {
    async fn sink_features(
        &mut self,
        token: &EdgeToken,
        features: ClientFeatures,
    ) -> EdgeResult<()> {
        self.sink_features(token, features);
        Ok(())
    }

    async fn fetch_features(&mut self, _token: &EdgeToken) -> EdgeResult<ClientFeaturesResponse> {
        todo!()
    }
}

#[cfg(test)]
mod test {
    use std::str::FromStr;
    use tokio::sync::mpsc;
    use unleash_types::client_features::ClientFeature;

    use super::*;

    #[tokio::test]
    async fn memory_provider_correctly_deduplicates_tokens() {
        let (send, _) = mpsc::channel::<EdgeToken>(32);
        let mut provider = MemoryProvider::new(send);
        let _ = provider
            .sink_tokens(vec![EdgeToken {
                token: "some_secret".into(),
                ..EdgeToken::default()
            }])
            .await;

        let _ = provider
            .sink_tokens(vec![EdgeToken {
                token: "some_secret".into(),
                ..EdgeToken::default()
            }])
            .await;

        assert!(provider.get_known_tokens().await.unwrap().len() == 1);
    }

    #[tokio::test]
    async fn memory_provider_correctly_determines_token_to_be_valid() {
        let (send, _) = mpsc::channel::<EdgeToken>(32);
        let mut provider = MemoryProvider::new(send);
        let _ = provider
            .sink_tokens(vec![EdgeToken {
                token: "some_secret".into(),
                status: TokenValidationStatus::Validated,
                ..EdgeToken::default()
            }])
            .await;

        assert_eq!(
            provider
                .get_token_validation_status("some_secret")
                .await
                .unwrap(),
            TokenValidationStatus::Validated
        )
    }

    #[tokio::test]
    async fn memory_provider_yields_correct_response_for_token() {
        let (send, _) = mpsc::channel::<EdgeToken>(32);

        let mut provider = MemoryProvider::new(send);
        let token = EdgeToken {
            token: "some-secret".into(),
            ..EdgeToken::default()
        };

        let features = ClientFeatures {
            version: 1,
            features: vec![ClientFeature {
                name: "James Bond".into(),
                ..ClientFeature::default()
            }],
            segments: None,
            query: None,
        };

        provider.sink_features(&token, features);

        let found_feature = provider.get_client_features(&token).await.unwrap().features[0].clone();
        assert!(found_feature.name == *"James Bond");
    }

    #[tokio::test]
    async fn memory_provider_can_yield_list_of_validated_tokens() {
        let james_bond = EdgeToken {
            status: TokenValidationStatus::Validated,
            ..EdgeToken::from_str("jamesbond").unwrap()
        };
        let frank_drebin = EdgeToken {
            status: TokenValidationStatus::Validated,
            ..EdgeToken::from_str("frankdrebin").unwrap()
        };

        let (send, _) = mpsc::channel::<EdgeToken>(32);
        let mut provider = MemoryProvider::new(send);
        let _ = provider
            .sink_tokens(vec![james_bond.clone(), frank_drebin.clone()])
            .await;
        let valid_tokens = provider
            .filter_valid_tokens(vec![
                "jamesbond".into(),
                "anotherinvalidone".into(),
                "frankdrebin".into(),
            ])
            .await
            .unwrap();
        assert_eq!(valid_tokens.len(), 2);
        assert!(valid_tokens.iter().any(|t| t.token == james_bond.token));
        assert!(valid_tokens.iter().any(|t| t.token == frank_drebin.token));
    }
}
