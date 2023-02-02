use std::collections::HashMap;

use async_trait::async_trait;
use dashmap::DashMap;
use unleash_types::client_features::ClientFeatures;

use crate::{
    error::EdgeError,
    types::{EdgeProvider, EdgeResult, EdgeSink, EdgeToken, FeaturesProvider, TokenProvider},
};

#[derive(Debug, Clone, Default)]
pub struct MemoryProvider {
    data_store: DashMap<String, ClientFeatures>,
    token_store: Vec<EdgeToken>,
}

impl MemoryProvider {
    fn sink_features(&mut self, token: &EdgeToken, features: ClientFeatures) {
        self.data_store.insert(token.token.clone(), features);
    }

    fn sink_tokens(&mut self, tokens: Vec<EdgeToken>) {
        let joined_tokens = tokens.iter().chain(self.token_store.iter());
        let deduplicated: HashMap<String, EdgeToken> = joined_tokens
            .map(|x| (x.token.clone(), x.clone()))
            .collect();
        self.token_store = deduplicated.into_values().collect();
    }
}

impl EdgeProvider for MemoryProvider {}

impl FeaturesProvider for MemoryProvider {
    fn get_client_features(&self, token: &EdgeToken) -> EdgeResult<ClientFeatures> {
        self.data_store
            .get(&token.token)
            .map(|v| v.value().clone())
            .ok_or_else(|| EdgeError::DataSourceError("Token not found".to_string()))
    }
}

impl TokenProvider for MemoryProvider {
    fn get_known_tokens(&self) -> EdgeResult<Vec<EdgeToken>> {
        Ok(self.token_store.clone())
    }

    fn secret_is_valid(&self, secret: &str) -> EdgeResult<bool> {
        Ok(self.get_known_tokens()?.iter().any(|t| t.token == secret))
    }

    fn token_details(&self, secret: String) -> EdgeResult<Option<EdgeToken>> {
        let tokens = self.get_known_tokens()?;
        Ok(tokens.into_iter().find(|t| t.token == secret))
    }
}

#[async_trait]
impl EdgeSink for MemoryProvider {
    async fn sink_features(
        &mut self,
        token: &EdgeToken,
        features: ClientFeatures,
    ) -> EdgeResult<()> {
        self.sink_features(token, features);
        Ok(())
    }

    async fn sink_tokens(&mut self, tokens: Vec<EdgeToken>) -> EdgeResult<()> {
        self.sink_tokens(tokens);
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use unleash_types::client_features::ClientFeature;

    use super::*;

    #[test]
    fn memory_provider_correctly_deduplicates_tokens() {
        let mut provider = MemoryProvider::default();
        provider.sink_tokens(vec![EdgeToken {
            token: "some_secret".into(),
            ..EdgeToken::default()
        }]);

        provider.sink_tokens(vec![EdgeToken {
            token: "some_secret".into(),
            ..EdgeToken::default()
        }]);

        assert!(provider.get_known_tokens().unwrap().len() == 1);
    }

    #[test]
    fn memory_provider_correctly_determines_token_to_be_valid() {
        let mut provider = MemoryProvider::default();
        provider.sink_tokens(vec![EdgeToken {
            token: "some_secret".into(),
            ..EdgeToken::default()
        }]);

        assert!(provider.secret_is_valid("some_secret").unwrap())
    }

    #[test]
    fn memory_provider_yields_correct_response_for_token() {
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

        let found_feature = provider.get_client_features(&token).unwrap().features[0].clone();
        assert!(found_feature.name == *"James Bond");
    }
}
