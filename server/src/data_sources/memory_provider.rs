use std::collections::HashMap;

use crate::types::{
    EdgeResult, EdgeSink, EdgeSource, EdgeToken, FeatureSink, FeaturesSource, TokenSink,
    TokenSource,
};
use crate::types::{FeatureRefresh, TokenValidationStatus};
use actix_web::http::header::EntityTag;
use async_trait::async_trait;
use chrono::{Duration, Utc};
use dashmap::DashMap;
use tracing::debug;
use unleash_types::client_features::ClientFeatures;
use unleash_types::Merge;

use super::ProjectFilter;

#[derive(Debug, Clone)]
pub struct MemoryProvider {
    features_refresh_interval: Duration,
    data_store: DashMap<String, ClientFeatures>,
    token_store: HashMap<String, EdgeToken>,
    tokens_to_refresh: HashMap<String, FeatureRefresh>,
}

fn key(key: &EdgeToken) -> String {
    key.environment.clone().unwrap()
}
impl Default for MemoryProvider {
    fn default() -> Self {
        Self::new(10)
    }
}
impl MemoryProvider {
    pub fn new(features_refresh_interval_seconds: i64) -> Self {
        Self {
            features_refresh_interval: Duration::seconds(features_refresh_interval_seconds),
            data_store: DashMap::new(),
            token_store: HashMap::new(),
            tokens_to_refresh: HashMap::new(),
        }
    }

    fn update_last_check(&mut self, token: &EdgeToken) {
        self.tokens_to_refresh
            .entry(token.token.clone())
            .and_modify(|f| f.last_check = Some(Utc::now()));
    }
    fn sink_tokens(&mut self, tokens: Vec<EdgeToken>) {
        for token in tokens {
            self.token_store.insert(token.token.clone(), token.clone());
            if token.token_type == Some(crate::types::TokenType::Client) {
                self.tokens_to_refresh
                    .entry(token.clone().token)
                    .or_insert(FeatureRefresh::new(token.clone()));
            }
        }
        self.reduce_tokens_to_refresh();
    }
    fn reduce_tokens_to_refresh(&mut self) {
        let tokens: Vec<EdgeToken> = self
            .tokens_to_refresh
            .values()
            .map(|r| r.token.clone())
            .collect();
        let minimized = crate::tokens::simplify(&tokens);
        self.tokens_to_refresh
            .retain(|k, _| minimized.iter().any(|m| &m.token == k));
    }
    fn sink_features(
        &mut self,
        token: &EdgeToken,
        features: ClientFeatures,
        etag: Option<EntityTag>,
    ) {
        debug!("Sinking features");
        self.tokens_to_refresh
            .entry(token.token.clone())
            .and_modify(|feature_refresh| {
                feature_refresh.etag = etag.clone();
                feature_refresh.last_refreshed = Some(Utc::now());
                feature_refresh.last_check = Some(Utc::now());
            })
            .or_insert(FeatureRefresh {
                token: token.clone(),
                etag,
                last_refreshed: Some(Utc::now()),
                last_check: Some(Utc::now()),
            });
        self.data_store
            .entry(key(token))
            .and_modify(|data| {
                data.clone().merge(features.clone());
            })
            .or_insert(features);
    }
}

impl EdgeSource for MemoryProvider {}
impl EdgeSink for MemoryProvider {}

pub fn empty_client_features() -> ClientFeatures {
    ClientFeatures {
        version: 2,
        features: vec![],
        segments: None,
        query: None,
    }
}

#[async_trait]
impl TokenSink for MemoryProvider {
    async fn sink_tokens(&mut self, tokens: Vec<EdgeToken>) -> EdgeResult<()> {
        self.sink_tokens(tokens);
        Ok(())
    }
}

#[async_trait]
impl FeaturesSource for MemoryProvider {
    async fn get_client_features(&self, token: &EdgeToken) -> EdgeResult<ClientFeatures> {
        let token_to_use = self
            .token_details(token.token.clone())
            .await?
            .unwrap_or(token.clone());
        let environment_features = self
            .data_store
            .get(&key(&token_to_use))
            .map(|v| v.value().clone());
        Ok(environment_features
            .map(|client_features| ClientFeatures {
                features: client_features.features.filter_by_projects(&token_to_use),
                ..client_features
            })
            .unwrap_or_else(empty_client_features))
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

    async fn get_tokens_due_for_refresh(&self) -> EdgeResult<Vec<FeatureRefresh>> {
        let refreshes = self
            .tokens_to_refresh
            .iter()
            .filter(|(_k, value)| match value.last_check {
                Some(last) => Utc::now() - last > self.features_refresh_interval,
                None => {
                    debug!("No last check date, definitely need to update this");
                    true
                }
            })
            .map(|(_k, refresh)| refresh.clone())
            .collect();
        Ok(refreshes)
    }
}

#[async_trait]
impl FeatureSink for MemoryProvider {
    async fn sink_features(
        &mut self,
        token: &EdgeToken,
        features: ClientFeatures,
        etag: Option<EntityTag>,
    ) -> EdgeResult<()> {
        self.sink_features(token, features, etag);
        Ok(())
    }
    async fn update_last_check(&mut self, token: &EdgeToken) -> EdgeResult<()> {
        self.update_last_check(token);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::types::into_entity_tag;
    use crate::types::TokenType;
    use std::str::FromStr;
    use unleash_types::client_features::ClientFeature;

    use super::*;

    #[tokio::test]
    async fn memory_provider_correctly_deduplicates_tokens() {
        let mut provider = MemoryProvider::default();
        provider.sink_tokens(vec![EdgeToken {
            token: "*:development.1d38eefdd7bf72676122b008dcf330f2f2aa2f3031438e1b7e8f0d1f".into(),
            ..EdgeToken::default()
        }]);

        provider.sink_tokens(vec![EdgeToken {
            token: "*:development.1d38eefdd7bf72676122b008dcf330f2f2aa2f3031438e1b7e8f0d1f".into(),
            ..EdgeToken::default()
        }]);

        assert!(provider.get_known_tokens().await.unwrap().len() == 1);
    }

    #[tokio::test]
    async fn memory_provider_correctly_determines_token_to_be_valid() {
        let mut provider = MemoryProvider::default();
        provider.sink_tokens(vec![EdgeToken {
            token: "*:development.1d38eefdd7bf72676122b008dcf330f2f2aa2f3031438e1b7e8f0d1f".into(),
            status: TokenValidationStatus::Validated,
            ..EdgeToken::default()
        }]);

        assert_eq!(
            provider
                .token_details(
                    "*:development.1d38eefdd7bf72676122b008dcf330f2f2aa2f3031438e1b7e8f0d1f".into()
                )
                .await
                .expect("Could not retrieve token details")
                .unwrap()
                .status,
            TokenValidationStatus::Validated
        )
    }

    #[tokio::test]
    async fn memory_provider_yields_correct_response_for_token() {
        let mut provider = MemoryProvider::default();
        let token = EdgeToken {
            environment: Some("development".into()),
            projects: vec!["default".into()],
            token: "*:development.1d38eefdd7bf72676122b008dcf330f2f2aa2f3031438e1b7e8f0d1ft".into(),
            ..EdgeToken::default()
        };

        let features = ClientFeatures {
            version: 1,
            features: vec![ClientFeature {
                name: "James Bond".into(),
                project: Some("default".into()),
                ..ClientFeature::default()
            }],
            segments: None,
            query: None,
        };

        provider.sink_features(&token, features.clone(), into_entity_tag(features));

        let found_feature = provider.get_client_features(&token).await.unwrap().features[0].clone();
        assert!(found_feature.name == *"James Bond");
    }

    #[tokio::test]
    async fn memory_provider_can_yield_list_of_validated_tokens() {
        let james_bond = EdgeToken {
            status: TokenValidationStatus::Validated,
            ..EdgeToken::from_str("*:development.jamesbond").unwrap()
        };
        let frank_drebin = EdgeToken {
            status: TokenValidationStatus::Validated,
            ..EdgeToken::from_str("*:development.frankdrebin").unwrap()
        };

        let mut provider = MemoryProvider::default();
        provider.sink_tokens(vec![james_bond.clone(), frank_drebin.clone()]);
        let valid_tokens = provider
            .filter_valid_tokens(vec![
                "*:development.jamesbond".into(),
                "*:development.anotherinvalidone".into(),
                "*:development.frankdrebin".into(),
            ])
            .await
            .unwrap();
        assert_eq!(valid_tokens.len(), 2);
        assert!(valid_tokens.iter().any(|t| t.token == james_bond.token));
        assert!(valid_tokens.iter().any(|t| t.token == frank_drebin.token));
    }

    #[tokio::test]
    async fn memory_provider_filters_out_features_by_token() {
        let mut provider = MemoryProvider::default();
        let token = EdgeToken {
            environment: Some("development".into()),
            projects: vec!["default".into()],
            token: "some-secret".into(),
            ..EdgeToken::default()
        };

        let features = ClientFeatures {
            version: 1,
            features: vec![
                ClientFeature {
                    name: "James Bond".into(),
                    project: Some("default".into()),
                    ..ClientFeature::default()
                },
                ClientFeature {
                    name: "Jason Bourne".into(),
                    project: Some("some-test-project".into()),
                    ..ClientFeature::default()
                },
            ],
            segments: None,
            query: None,
        };

        provider.sink_features(&token, features.clone(), into_entity_tag(features));

        let all_features = provider.get_client_features(&token).await.unwrap().features;
        let found_feature = all_features[0].clone();

        assert!(all_features.len() == 1);
        assert!(found_feature.name == *"James Bond");
    }

    #[tokio::test]
    async fn memory_provider_respects_all_projects_in_token() {
        let mut provider = MemoryProvider::default();
        let token = EdgeToken {
            environment: Some("development".into()),
            projects: vec!["*".into()],
            token: "some-secret".into(),
            ..EdgeToken::default()
        };

        let features = ClientFeatures {
            version: 1,
            features: vec![
                ClientFeature {
                    name: "James Bond".into(),
                    project: Some("default".into()),
                    ..ClientFeature::default()
                },
                ClientFeature {
                    name: "Jason Bourne".into(),
                    project: Some("some-test-project".into()),
                    ..ClientFeature::default()
                },
            ],
            segments: None,
            query: None,
        };

        provider.sink_features(&token, features.clone(), into_entity_tag(features));

        let all_features = provider.get_client_features(&token).await.unwrap().features;
        let first_feature = all_features
            .iter()
            .find(|x| x.name == "James Bond")
            .unwrap();

        let second_feature = all_features
            .iter()
            .find(|x| x.name == "Jason Bourne")
            .unwrap();

        assert!(all_features.len() == 2);
        assert!(first_feature.name == *"James Bond");
        assert!(second_feature.name == *"Jason Bourne");
    }

    #[tokio::test]
    pub async fn can_minimize_tokens_to_check() {
        let mut memory_provider = MemoryProvider::default();
        let mut token_for_default_project =
            EdgeToken::try_from("default:development.1234567890123456".to_string()).unwrap();
        token_for_default_project.token_type = Some(TokenType::Client);
        let mut token_for_test_project =
            EdgeToken::try_from("test:development.abcdefghijklmnopqerst".to_string()).unwrap();
        token_for_test_project.token_type = Some(TokenType::Client);
        memory_provider.sink_tokens(vec![token_for_test_project, token_for_default_project]);
        assert_eq!(memory_provider.tokens_to_refresh.len(), 2);
        let mut wildcard_development_token =
            EdgeToken::try_from("*:development.12321jwewhrkvkjewlrkjwqlkrjw".to_string()).unwrap();
        wildcard_development_token.token_type = Some(TokenType::Client);
        memory_provider.sink_tokens(vec![wildcard_development_token.clone()]);
        assert_eq!(memory_provider.tokens_to_refresh.len(), 1);
        assert!(memory_provider
            .tokens_to_refresh
            .contains_key(&wildcard_development_token.token));
    }
}
