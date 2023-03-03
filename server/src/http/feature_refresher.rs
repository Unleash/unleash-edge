use std::{sync::Arc, time::Duration};

use actix_web::http::header::EntityTag;
use chrono::Utc;
use dashmap::DashMap;
use std::collections::HashSet;
use tracing::{debug, warn};
use unleash_types::{client_features::ClientFeatures, Upsert};
use unleash_yggdrasil::EngineState;

use crate::{
    tokens::{cache_key, simplify},
    types::{ClientFeaturesRequest, ClientFeaturesResponse, EdgeToken, TokenRefresh},
};

use super::unleash_client::UnleashClient;

#[derive(Clone)]
pub struct FeatureRefresher {
    pub unleash_client: Arc<UnleashClient>,
    pub tokens_to_refresh: DashMap<String, TokenRefresh>,
    pub features_cache: Arc<DashMap<String, ClientFeatures>>,
    pub engine_cache: Arc<DashMap<String, EngineState>>,
    pub refresh_interval: chrono::Duration,
}

impl FeatureRefresher {
    pub fn new(
        unleash_client: Arc<UnleashClient>,
        features: Arc<DashMap<String, ClientFeatures>>,
        engines: Arc<DashMap<String, EngineState>>,
        features_refresh_interval: chrono::Duration,
    ) -> Self {
        FeatureRefresher {
            unleash_client,
            tokens_to_refresh: DashMap::default(),
            features_cache: features,
            engine_cache: engines,
            refresh_interval: features_refresh_interval,
        }
    }
    pub fn get_tokens_due_for_refresh(&self) -> Vec<TokenRefresh> {
        self.tokens_to_refresh
            .iter()
            .map(|e| e.value().clone())
            .filter(|token| {
                token
                    .last_check
                    .map(|last| Utc::now() - last > self.refresh_interval)
                    .unwrap_or(true)
            })
            .collect()
    }

    pub fn register_token_for_refresh(&self, token: EdgeToken) {
        if !self.tokens_to_refresh.contains_key(&token.token) {
            let mut registered_tokens: Vec<TokenRefresh> =
                self.tokens_to_refresh.iter().map(|t| t.clone()).collect();
            registered_tokens.push(TokenRefresh::new(token));
            let minimum = simplify(&registered_tokens);
            let mut keys = HashSet::new();
            for token in minimum {
                keys.insert(token.token.token.clone());
                self.tokens_to_refresh
                    .insert(token.token.token.clone(), token.clone());
            }
            self.tokens_to_refresh.retain(|key, _| keys.contains(key));
        }
    }

    pub async fn refresh_features(&self) {
        loop {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(5)) => {
                    let refreshes = self.get_tokens_due_for_refresh();
                        for refresh in refreshes {
                            let features_result = self.unleash_client.get_client_features(ClientFeaturesRequest {
                                api_key: refresh.token.token.clone(),
                                etag: refresh.etag
                            }).await;

                            match features_result {
                                Ok(feature_response)  => match feature_response {
                                    ClientFeaturesResponse::NoUpdate(_) => {
                                        debug!("No update needed. Will update last check time");
                                        self.update_last_check(&refresh.token.clone());
                                    }
                                    ClientFeaturesResponse::Updated(features, etag) => {
                                        debug!("Got updated client features. Updating features");
                                        let key = cache_key(refresh.token.clone());
                                        self.update_last_refresh(&refresh.token, etag);
                                        self.features_cache.entry(key.clone()).and_modify(|existing_data| {
                                            *existing_data = existing_data.clone().upsert(features.clone());
                                        }).or_insert(features.clone());
                                        if self.engine_cache.contains_key(&key) {
                                            self.engine_cache.entry(key.clone()).and_modify(|engine| {
                                                if let Some(f) = self.features_cache.get(&key) {
                                                        let mut new_state = EngineState::default();
                                                        new_state.take_state(f.clone());
                                                        *engine = new_state;
                                                }
                                            });
                                        } else {
                                            let mut new_state = EngineState::default();
                                            new_state.take_state(features.clone());
                                            self.engine_cache.insert(key, new_state);
                                        }
                                    }
                                },
                                Err(e) => {
                                    warn!("Couldn't refresh features: {e:?}");
                                }
                            }
                        }

                }
            }
        }
    }

    pub fn update_last_check(&self, token: &EdgeToken) {
        if let Some(mut token) = self.tokens_to_refresh.get_mut(&token.token) {
            token.last_check = Some(chrono::Utc::now());
        }
    }

    pub fn update_last_refresh(&self, token: &EdgeToken, etag: Option<EntityTag>) {
        self.tokens_to_refresh
            .entry(token.token.clone())
            .and_modify(|token_to_refresh| {
                token_to_refresh.last_check = Some(chrono::Utc::now());
                token_to_refresh.last_refreshed = Some(chrono::Utc::now());
                token_to_refresh.etag = etag
            });
    }
}

#[cfg(test)]
mod tests {
    use actix_web::http::header::EntityTag;
    use chrono::{Duration, Utc};
    use std::sync::Arc;

    use dashmap::DashMap;
    use reqwest::Url;

    use crate::{
        http::unleash_client::UnleashClient,
        types::{EdgeToken, TokenRefresh},
    };

    use super::FeatureRefresher;

    impl PartialEq for TokenRefresh {
        fn eq(&self, other: &Self) -> bool {
            self.token == other.token
                && self.etag == other.etag
                && self.last_refreshed == other.last_refreshed
                && self.last_check == other.last_check
        }
    }

    #[test]
    pub fn registering_token_for_refresh_works() {
        let unleash_client = UnleashClient::from_url(Url::parse("http://localhost:4242").unwrap());
        let features_cache = Arc::new(DashMap::default());
        let engines_cache = Arc::new(DashMap::default());

        let duration = Duration::seconds(5);
        let feature_refresher = FeatureRefresher::new(
            Arc::new(unleash_client),
            features_cache,
            engines_cache,
            duration,
        );
        let token =
            EdgeToken::try_from("*:development.abcdefghijklmnopqrstuvwxyz".to_string()).unwrap();
        feature_refresher.register_token_for_refresh(token);

        assert_eq!(feature_refresher.tokens_to_refresh.len(), 1);
    }

    #[test]
    pub fn registering_multiple_non_overlapping_tokens_will_keep_all() {
        let unleash_client = UnleashClient::from_url(Url::parse("http://localhost:4242").unwrap());
        let features_cache = Arc::new(DashMap::default());
        let engines_cache = Arc::new(DashMap::default());
        let duration = Duration::seconds(5);
        let feature_refresher = FeatureRefresher::new(
            Arc::new(unleash_client),
            features_cache,
            engines_cache,
            duration,
        );
        let project_a_token =
            EdgeToken::try_from("projecta:development.abcdefghijklmnopqrstuvwxyz".to_string())
                .unwrap();
        let project_b_token =
            EdgeToken::try_from("projectb:development.abcdefghijklmnopqrstuvwxyz".to_string())
                .unwrap();
        let project_c_token =
            EdgeToken::try_from("projectc:development.abcdefghijklmnopqrstuvwxyz".to_string())
                .unwrap();
        feature_refresher.register_token_for_refresh(project_a_token);
        feature_refresher.register_token_for_refresh(project_b_token);
        feature_refresher.register_token_for_refresh(project_c_token);

        assert_eq!(feature_refresher.tokens_to_refresh.len(), 3);
    }

    #[test]
    pub fn registering_wildcard_project_token_only_keeps_the_wildcard() {
        let unleash_client = UnleashClient::from_url(Url::parse("http://localhost:4242").unwrap());
        let features_cache = Arc::new(DashMap::default());
        let engines_cache = Arc::new(DashMap::default());
        let duration = Duration::seconds(5);
        let feature_refresher = FeatureRefresher::new(
            Arc::new(unleash_client),
            features_cache,
            engines_cache,
            duration,
        );
        let project_a_token =
            EdgeToken::try_from("projecta:development.abcdefghijklmnopqrstuvwxyz".to_string())
                .unwrap();
        let project_b_token =
            EdgeToken::try_from("projectb:development.abcdefghijklmnopqrstuvwxyz".to_string())
                .unwrap();
        let project_c_token =
            EdgeToken::try_from("projectc:development.abcdefghijklmnopqrstuvwxyz".to_string())
                .unwrap();
        let wildcard_token =
            EdgeToken::try_from("*:development.abcdefghijklmnopqrstuvwxyz".to_string()).unwrap();

        feature_refresher.register_token_for_refresh(project_a_token);
        feature_refresher.register_token_for_refresh(project_b_token);
        feature_refresher.register_token_for_refresh(project_c_token);
        feature_refresher.register_token_for_refresh(wildcard_token);

        assert_eq!(feature_refresher.tokens_to_refresh.len(), 1);
        assert!(feature_refresher
            .tokens_to_refresh
            .contains_key("*:development.abcdefghijklmnopqrstuvwxyz"))
    }

    #[test]
    pub fn registering_tokens_with_multiple_projects_overwrites_single_tokens() {
        let unleash_client = UnleashClient::from_url(Url::parse("http://localhost:4242").unwrap());
        let features_cache = Arc::new(DashMap::default());
        let engines_cache = Arc::new(DashMap::default());
        let duration = Duration::seconds(5);
        let feature_refresher = FeatureRefresher::new(
            Arc::new(unleash_client),
            features_cache,
            engines_cache,
            duration,
        );
        let project_a_token =
            EdgeToken::try_from("projecta:development.abcdefghijklmnopqrstuvwxyz".to_string())
                .unwrap();
        let project_b_token =
            EdgeToken::try_from("projectb:development.abcdefghijklmnopqrstuvwxyz".to_string())
                .unwrap();
        let project_c_token =
            EdgeToken::try_from("projectc:development.abcdefghijklmnopqrstuvwxyz".to_string())
                .unwrap();
        let mut project_a_and_c_token =
            EdgeToken::try_from("[]:development.abcdefghijklmnopqrstuvwxyz".to_string()).unwrap();
        project_a_and_c_token.projects = vec!["projecta".into(), "projectc".into()];

        feature_refresher.register_token_for_refresh(project_a_token);
        feature_refresher.register_token_for_refresh(project_b_token);
        feature_refresher.register_token_for_refresh(project_c_token);
        feature_refresher.register_token_for_refresh(project_a_and_c_token);

        assert_eq!(feature_refresher.tokens_to_refresh.len(), 2);
        assert!(feature_refresher
            .tokens_to_refresh
            .contains_key("[]:development.abcdefghijklmnopqrstuvwxyz"));
        assert!(feature_refresher
            .tokens_to_refresh
            .contains_key("projectb:development.abcdefghijklmnopqrstuvwxyz"));
    }

    #[test]
    pub fn registering_a_token_that_is_already_subsumed_does_nothing() {
        let unleash_client = UnleashClient::from_url(Url::parse("http://localhost:4242").unwrap());
        let features_cache = Arc::new(DashMap::default());
        let engines_cache = Arc::new(DashMap::default());

        let duration = Duration::seconds(5);
        let feature_refresher = FeatureRefresher::new(
            Arc::new(unleash_client),
            features_cache,
            engines_cache,
            duration,
        );
        let star_token =
            EdgeToken::try_from("*:development.abcdefghijklmnopqrstuvwxyz".to_string()).unwrap();
        let project_a_token =
            EdgeToken::try_from("projecta:development.abcdefghijklmnopqrstuvwxyz".to_string())
                .unwrap();

        feature_refresher.register_token_for_refresh(star_token);
        feature_refresher.register_token_for_refresh(project_a_token);

        assert_eq!(feature_refresher.tokens_to_refresh.len(), 1);
        assert!(feature_refresher
            .tokens_to_refresh
            .contains_key("*:development.abcdefghijklmnopqrstuvwxyz"));
    }

    #[test]
    pub fn simplification_only_happens_in_same_environment() {
        let unleash_client = UnleashClient::from_url(Url::parse("http://localhost:4242").unwrap());
        let features_cache = Arc::new(DashMap::default());
        let engines_cache = Arc::new(DashMap::default());

        let duration = Duration::seconds(5);
        let feature_refresher = FeatureRefresher::new(
            Arc::new(unleash_client),
            features_cache,
            engines_cache,
            duration,
        );
        let project_a_token =
            EdgeToken::try_from("projecta:development.abcdefghijklmnopqrstuvwxyz".to_string())
                .unwrap();
        let production_wildcard_token =
            EdgeToken::try_from("*:production.abcdefghijklmnopqrstuvwxyz".to_string()).unwrap();
        feature_refresher.register_token_for_refresh(project_a_token);
        feature_refresher.register_token_for_refresh(production_wildcard_token);
        assert_eq!(feature_refresher.tokens_to_refresh.len(), 2);
    }

    #[test]
    pub fn is_able_to_only_fetch_for_tokens_due_to_refresh() {
        let unleash_client = UnleashClient::from_url(Url::parse("http://localhost:4242").unwrap());
        let features_cache = Arc::new(DashMap::default());
        let engines_cache = Arc::new(DashMap::default());

        let duration = Duration::seconds(5);
        let feature_refresher = FeatureRefresher::new(
            Arc::new(unleash_client),
            features_cache,
            engines_cache,
            duration,
        );
        let no_etag_due_for_refresh_token =
            EdgeToken::try_from("projecta:development.no_etag_due_for_refresh_token".to_string())
                .unwrap();
        let no_etag_so_is_due_for_refresh = TokenRefresh {
            token: no_etag_due_for_refresh_token,
            etag: None,
            last_refreshed: None,
            last_check: None,
        };
        let etag_and_last_refreshed_token =
            EdgeToken::try_from("projectb:development.etag_and_last_refreshed_token".to_string())
                .unwrap();
        let etag_and_last_refreshed_less_than_duration_ago = TokenRefresh {
            token: etag_and_last_refreshed_token,
            etag: Some(EntityTag::new_weak("abcde".into())),
            last_refreshed: Some(Utc::now()),
            last_check: Some(Utc::now()),
        };
        let etag_but_old_token =
            EdgeToken::try_from("projectb:development.etag_but_old_token".to_string()).unwrap();

        let ten_seconds_ago = Utc::now() - Duration::seconds(10);
        let etag_but_last_refreshed_ten_seconds_ago = TokenRefresh {
            token: etag_but_old_token,
            etag: Some(EntityTag::new_weak("abcde".into())),
            last_refreshed: Some(ten_seconds_ago),
            last_check: Some(ten_seconds_ago),
        };
        feature_refresher.tokens_to_refresh.insert(
            etag_but_last_refreshed_ten_seconds_ago.token.token.clone(),
            etag_but_last_refreshed_ten_seconds_ago.clone(),
        );
        feature_refresher.tokens_to_refresh.insert(
            etag_and_last_refreshed_less_than_duration_ago
                .token
                .token
                .clone(),
            etag_and_last_refreshed_less_than_duration_ago,
        );
        feature_refresher.tokens_to_refresh.insert(
            no_etag_so_is_due_for_refresh.token.token.clone(),
            no_etag_so_is_due_for_refresh.clone(),
        );
        let tokens_to_refresh = feature_refresher.get_tokens_due_for_refresh();
        assert_eq!(tokens_to_refresh.len(), 2);
        assert!(tokens_to_refresh.contains(&etag_but_last_refreshed_ten_seconds_ago));
        assert!(tokens_to_refresh.contains(&no_etag_so_is_due_for_refresh));
    }
}
