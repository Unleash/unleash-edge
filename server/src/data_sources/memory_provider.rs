use std::{collections::HashMap, sync::Arc};

use crate::{
    http::unleash_client::UnleashClient,
    types::{ClientFeaturesRequest, ClientFeaturesResponse},
};
use async_trait::async_trait;
use dashmap::DashMap;
use std::str::FromStr;
use tokio::sync::mpsc::Sender;
use unleash_types::client_features::ClientFeatures;

use crate::{
    error::EdgeError,
    types::{
        EdgeProvider, EdgeResult, EdgeSink, EdgeSource, EdgeToken, FeatureSink, FeaturesSource,
        TokenSink, TokenSource, ValidateTokensRequest,
    },
};

#[derive(Debug, Clone, Default)]
pub struct MemoryProvider {
    data_store: DashMap<String, ClientFeatures>,
    token_store: HashMap<String, EdgeToken>,
    unleash_client: UnleashClient,
}

impl MemoryProvider {
    fn sink_features(&mut self, token: &EdgeToken, features: ClientFeatures) {
        self.data_store.insert(token.token.clone(), features);
    }
}
impl EdgeProvider for MemoryProvider {}
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

    async fn validate(&mut self, tokens: Vec<EdgeToken>) -> EdgeResult<Vec<EdgeToken>> {
        let validation_request = ValidateTokensRequest {
            tokens: tokens.into_iter().map(|t| t.token).collect(),
        };
        self.unleash_client
            .validate_tokens(validation_request)
            .await
    }
}

#[async_trait]
impl FeaturesSource for MemoryProvider {
    async fn get_client_features(&self, token: &EdgeToken) -> EdgeResult<ClientFeatures> {
        self.data_store
            .get(&token.token)
            .map(|v| v.value().clone())
            .ok_or_else(|| EdgeError::DataSourceError("Token not found".to_string()))
    }
}

#[async_trait]
impl TokenSource for MemoryProvider {
    async fn get_known_tokens(&self) -> EdgeResult<Vec<EdgeToken>> {
        Ok(self.token_store.values().into_iter().cloned().collect())
    }

    async fn secret_is_valid(
        &self,
        secret: &str,
        sender: Arc<Sender<EdgeToken>>,
    ) -> EdgeResult<bool> {
        if self
            .get_known_tokens()
            .await?
            .iter()
            .any(|t| t.token == secret)
        {
            Ok(true)
        } else {
            let _ = sender.send(EdgeToken::try_from(secret.to_string())?).await;
            Ok(false)
        }
    }

    async fn token_details(&self, secret: String) -> EdgeResult<Option<EdgeToken>> {
        let tokens = self.get_known_tokens().await?;
        Ok(tokens.into_iter().find(|t| t.token == secret))
    }

    async fn get_valid_tokens(&self, secrets: Vec<String>) -> EdgeResult<Vec<EdgeToken>> {
        let tokens = self.get_known_tokens().await?;
        Ok(secrets
            .into_iter()
            .map(|s| EdgeToken::from_str(s.as_str()).unwrap())
            .filter(|t| tokens.iter().any(|valid| valid.token == t.token))
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

    async fn fetch_features(&mut self, token: &EdgeToken) -> EdgeResult<ClientFeaturesResponse> {
        self.unleash_client
            .get_client_features(ClientFeaturesRequest {
                api_key: token.token.clone(),
                etag: None,
            })
            .await
    }
}

#[cfg(test)]
mod test {
    use std::sync::Arc;

    use tokio::sync::mpsc;
    use unleash_types::client_features::ClientFeature;

    use super::*;

    #[tokio::test]
    async fn memory_provider_correctly_deduplicates_tokens() {
        let mut provider = MemoryProvider::default();
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
        let mut provider = MemoryProvider::default();
        let _ = provider
            .sink_tokens(vec![EdgeToken {
                token: "some_secret".into(),
                ..EdgeToken::default()
            }])
            .await;

        let (send, _) = mpsc::channel::<EdgeToken>(32);

        assert!(provider
            .secret_is_valid("some_secret", Arc::new(send))
            .await
            .unwrap())
    }

    #[tokio::test]
    async fn memory_provider_yields_correct_response_for_token() {
        let mut provider = MemoryProvider::default();
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
        let james_bond = EdgeToken::from_str("jamesbond").unwrap();
        let frank_drebin = EdgeToken::from_str("frankdrebin").unwrap();

        let mut provider = MemoryProvider::default();
        let _ = provider
            .sink_tokens(vec![james_bond.clone(), frank_drebin.clone()])
            .await;
        let valid_tokens = provider
            .get_valid_tokens(vec![
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
