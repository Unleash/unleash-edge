use std::collections::HashMap;

use crate::types::TokenRefresh;
use crate::types::{EdgeResult, EdgeToken};
use actix_web::http::header::EntityTag;
use async_trait::async_trait;
use dashmap::DashMap;
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
    async fn get_tokens(&self) -> EdgeResult<Vec<EdgeToken>> {
        Ok(self.token_store.values().into_iter().cloned().collect())
    }

    async fn get_token(&self, secret: &str) -> EdgeResult<Option<EdgeToken>> {
        Ok(self.token_store.get(secret).cloned())
    }

    async fn get_refresh_tokens(&self) -> EdgeResult<Vec<TokenRefresh>> {
        Ok(self.tokens_to_refresh.values().cloned().collect())
    }

    async fn get_client_features(&self, token: &EdgeToken) -> EdgeResult<Option<ClientFeatures>> {
        Ok(self.data_store.get(&key(&token)).map(|v| v.value().clone()))
    }
}

#[async_trait]
impl DataSink for MemoryProvider {
    async fn sink_tokens(&mut self, tokens: Vec<EdgeToken>) -> EdgeResult<()> {
        for token in tokens {
            self.token_store.insert(token.token.clone(), token.clone());
        }
        Ok(())
    }

    async fn sink_refresh_tokens(&mut self, tokens: Vec<&TokenRefresh>) -> EdgeResult<()> {
        for token in tokens {
            self.tokens_to_refresh
                .entry(token.token.token.clone())
                .or_insert(token.clone());
        }
        Ok(())
    }

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

    async fn update_last_check(&mut self, token: &EdgeToken) -> EdgeResult<()> {
        if let Some(token) = self.tokens_to_refresh.get_mut(&token.token) {
            token.last_check = Some(chrono::Utc::now());
        }
        Ok(())
    }

    async fn update_last_refresh(&mut self, token: &EdgeToken, etag: Option<EntityTag>) -> EdgeResult<()> {
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
                .get_token(
                    "*:development.1d38eefdd7bf72676122b008dcf330f2f2aa2f3031438e1b7e8f0d1f".into()
                )
                .await
                .expect("Could not retrieve token details")
                .unwrap()
                .status,
            TokenValidationStatus::Validated
        )
    }

    // TODO: Maybe some of these tests belong to repository instead now?

    // #[tokio::test]
    // async fn memory_provider_yields_correct_response_for_token() {
    //     let mut provider = MemoryProvider::default();
    //     let token = EdgeToken {
    //         environment: Some("development".into()),
    //         projects: vec!["default".into()],
    //         token: "*:development.1d38eefdd7bf72676122b008dcf330f2f2aa2f3031438e1b7e8f0d1ft".into(),
    //         ..EdgeToken::default()
    //     };

    //     let features = ClientFeatures {
    //         version: 1,
    //         features: vec![ClientFeature {
    //             name: "James Bond".into(),
    //             project: Some("default".into()),
    //             ..ClientFeature::default()
    //         }],
    //         segments: None,
    //         query: None,
    //     };

    //     provider.sink_features(&token, features.clone(), into_entity_tag(features));

    //     let found_feature = provider.get_client_features(&token).await.unwrap().features[0].clone();
    //     assert!(found_feature.name == *"James Bond");
    // }

    // #[tokio::test]
    // async fn memory_provider_can_yield_list_of_validated_tokens() {
    //     let james_bond = EdgeToken {
    //         status: TokenValidationStatus::Validated,
    //         ..EdgeToken::from_str("*:development.jamesbond").unwrap()
    //     };
    //     let frank_drebin = EdgeToken {
    //         status: TokenValidationStatus::Validated,
    //         ..EdgeToken::from_str("*:development.frankdrebin").unwrap()
    //     };

    //     let mut provider = MemoryProvider::default();
    //     provider.sink_tokens(vec![james_bond.clone(), frank_drebin.clone()]);
    //     let valid_tokens = provider
    //         .filter_valid_tokens(vec![
    //             "*:development.jamesbond".into(),
    //             "*:development.anotherinvalidone".into(),
    //             "*:development.frankdrebin".into(),
    //         ])
    //         .await
    //         .unwrap();
    //     assert_eq!(valid_tokens.len(), 2);
    //     assert!(valid_tokens.iter().any(|t| t.token == james_bond.token));
    //     assert!(valid_tokens.iter().any(|t| t.token == frank_drebin.token));
    // }

    // #[tokio::test]
    // async fn memory_provider_filters_out_features_by_token() {
    //     let mut provider = MemoryProvider::default();
    //     let token = EdgeToken {
    //         environment: Some("development".into()),
    //         projects: vec!["default".into()],
    //         token: "some-secret".into(),
    //         ..EdgeToken::default()
    //     };

    //     let features = ClientFeatures {
    //         version: 1,
    //         features: vec![
    //             ClientFeature {
    //                 name: "James Bond".into(),
    //                 project: Some("default".into()),
    //                 ..ClientFeature::default()
    //             },
    //             ClientFeature {
    //                 name: "Jason Bourne".into(),
    //                 project: Some("some-test-project".into()),
    //                 ..ClientFeature::default()
    //             },
    //         ],
    //         segments: None,
    //         query: None,
    //     };

    //     provider.sink_features(&token, features.clone(), into_entity_tag(features));

    //     let all_features = provider.get_client_features(&token).await.unwrap().features;
    //     let found_feature = all_features[0].clone();

    //     assert!(all_features.len() == 1);
    //     assert!(found_feature.name == *"James Bond");
    // }

    // #[tokio::test]
    // async fn memory_provider_can_update_data() {
    //     let mut provider = MemoryProvider::default();
    //     let token = EdgeToken {
    //         environment: Some("development".into()),
    //         projects: vec!["default".into()],
    //         token: "some-secret".into(),
    //         ..EdgeToken::default()
    //     };

    //     let first_features = ClientFeatures {
    //         version: 1,
    //         features: vec![ClientFeature {
    //             name: "James Bond".into(),
    //             project: Some("default".into()),
    //             ..ClientFeature::default()
    //         }],
    //         segments: None,
    //         query: None,
    //     };
    //     let second_features = ClientFeatures {
    //         version: 1,
    //         features: vec![ClientFeature {
    //             name: "Jason Bourne".into(),
    //             project: Some("default".into()),
    //             ..ClientFeature::default()
    //         }],
    //         segments: None,
    //         query: None,
    //     };

    //     provider.sink_features(
    //         &token,
    //         second_features.clone(),
    //         into_entity_tag(second_features),
    //     );
    //     provider.sink_features(
    //         &token,
    //         first_features.clone(),
    //         into_entity_tag(first_features),
    //     );

    //     let all_features = provider.get_client_features(&token).await.unwrap().features;

    //     assert!(all_features.len() == 2);
    // }

    // #[tokio::test]
    // async fn memory_provider_respects_all_projects_in_token() {
    //     let mut provider = MemoryProvider::default();
    //     let token = EdgeToken {
    //         environment: Some("development".into()),
    //         projects: vec!["*".into()],
    //         token: "some-secret".into(),
    //         ..EdgeToken::default()
    //     };

    //     let features = ClientFeatures {
    //         version: 1,
    //         features: vec![
    //             ClientFeature {
    //                 name: "James Bond".into(),
    //                 project: Some("default".into()),
    //                 ..ClientFeature::default()
    //             },
    //             ClientFeature {
    //                 name: "Jason Bourne".into(),
    //                 project: Some("some-test-project".into()),
    //                 ..ClientFeature::default()
    //             },
    //         ],
    //         segments: None,
    //         query: None,
    //     };

    //     provider.sink_features(&token, features.clone(), into_entity_tag(features));

    //     let all_features = provider.get_client_features(&token).await.unwrap().features;
    //     let first_feature = all_features
    //         .iter()
    //         .find(|x| x.name == "James Bond")
    //         .unwrap();

    //     let second_feature = all_features
    //         .iter()
    //         .find(|x| x.name == "Jason Bourne")
    //         .unwrap();

    //     assert!(all_features.len() == 2);
    //     assert!(first_feature.name == *"James Bond");
    //     assert!(second_feature.name == *"Jason Bourne");
    // }
}
