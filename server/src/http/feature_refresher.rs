use std::{sync::Arc, time::Duration};

use super::unleash_client::UnleashClient;
use crate::error::{EdgeError, FeatureError};
use crate::types::{build, EdgeResult, ProjectFilter};
use crate::{
    persistence::EdgePersistence,
    tokens::{cache_key, simplify},
    types::{ClientFeaturesRequest, ClientFeaturesResponse, EdgeToken, TokenRefresh},
};
use actix_web::http::header::EntityTag;
use chrono::Utc;
use dashmap::DashMap;
use std::collections::HashSet;
use tracing::{debug, warn};
use unleash_types::client_metrics::ClientApplication;
use unleash_types::{client_features::ClientFeatures, Upsert};
use unleash_yggdrasil::EngineState;

#[derive(Clone)]
pub struct FeatureRefresher {
    pub unleash_client: Arc<UnleashClient>,
    pub tokens_to_refresh: Arc<DashMap<String, TokenRefresh>>,
    pub features_cache: Arc<DashMap<String, ClientFeatures>>,
    pub engine_cache: Arc<DashMap<String, EngineState>>,
    pub refresh_interval: chrono::Duration,
    pub persistence: Option<Arc<dyn EdgePersistence>>,
}

impl Default for FeatureRefresher {
    fn default() -> Self {
        Self {
            refresh_interval: chrono::Duration::seconds(10),
            unleash_client: Default::default(),
            tokens_to_refresh: Arc::new(DashMap::default()),
            features_cache: Default::default(),
            engine_cache: Default::default(),
            persistence: None,
        }
    }
}

fn client_application_from_token(token: EdgeToken, refresh_interval: i64) -> ClientApplication {
    ClientApplication {
        app_name: "unleash_edge".into(),
        connect_via: None,
        environment: token.environment,
        instance_id: None,
        interval: refresh_interval as u32,
        sdk_version: Some(format!("unleash-edge:{}", build::PKG_VERSION)),
        started: Utc::now(),
        strategies: vec![],
    }
}

impl FeatureRefresher {
    pub fn new(
        unleash_client: Arc<UnleashClient>,
        features: Arc<DashMap<String, ClientFeatures>>,
        engines: Arc<DashMap<String, EngineState>>,
        features_refresh_interval: chrono::Duration,
        persistence: Option<Arc<dyn EdgePersistence>>,
    ) -> Self {
        FeatureRefresher {
            unleash_client,
            tokens_to_refresh: Arc::new(DashMap::default()),
            features_cache: features,
            engine_cache: engines,
            refresh_interval: features_refresh_interval,
            persistence,
        }
    }

    pub fn with_client(client: Arc<UnleashClient>) -> Self {
        Self {
            unleash_client: client,
            tokens_to_refresh: Arc::new(Default::default()),
            features_cache: Arc::new(Default::default()),
            engine_cache: Arc::new(Default::default()),
            refresh_interval: chrono::Duration::seconds(10),
            persistence: None,
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

    pub fn get_tokens_never_refreshed(&self) -> Vec<TokenRefresh> {
        self.tokens_to_refresh
            .iter()
            .map(|e| e.value().clone())
            .filter(|token| token.last_refreshed.is_none() && token.last_check.is_none())
            .collect()
    }

    pub fn token_is_subsumed(&self, token: &EdgeToken) -> bool {
        self.tokens_to_refresh
            .iter()
            .filter(|r| r.value().token.environment == token.environment)
            .any(|t| t.token.subsumes(token))
    }

    async fn register_and_hydrate_token(&self, token: &EdgeToken) -> EdgeResult<ClientFeatures> {
        self.register_token_for_refresh(token.clone(), None).await?;
        self.hydrate_new_tokens().await;
        self.get_filtered_features(token)
            .ok_or(EdgeError::ClientFeaturesFetchError(FeatureError::Retriable))
    }

    pub async fn features_for_token(&self, token: EdgeToken) -> EdgeResult<ClientFeatures> {
        match self.get_filtered_features(&token) {
            Some(features) => {
                if self.token_is_subsumed(&token) {
                    Ok(features)
                } else {
                    debug!("Token is not subsumed by existing tokens. Registering");
                    self.register_and_hydrate_token(&token).await
                }
            }
            None => {
                debug!("Had never seen this environment. Configuring fetcher");
                self.register_and_hydrate_token(&token).await
            }
        }
    }

    fn get_filtered_features(&self, token: &EdgeToken) -> Option<ClientFeatures> {
        self.features_cache
            .get(&cache_key(token))
            .map(|e| e.value().clone())
            .map(|features_response| ClientFeatures {
                features: features_response.features.filter_by_projects(token),
                ..features_response
            })
    }

    ///
    /// Registers a token for refresh, returns true if token did not exist and as such needs hydration before we can guarantee that we have data for it
    pub async fn register_token_for_refresh(
        &self,
        token: EdgeToken,
        etag: Option<EntityTag>,
    ) -> EdgeResult<()> {
        if !self.tokens_to_refresh.contains_key(&token.token) {
            let _ = self
                .unleash_client
                .register_as_client(
                    token.token.clone(),
                    client_application_from_token(
                        token.clone(),
                        self.refresh_interval.num_seconds(),
                    ),
                )
                .await;
            let mut registered_tokens: Vec<TokenRefresh> =
                self.tokens_to_refresh.iter().map(|t| t.clone()).collect();
            registered_tokens.push(TokenRefresh::new(token.clone(), etag));
            let minimum = simplify(&registered_tokens);
            let mut keys = HashSet::new();
            for refreshes in minimum {
                keys.insert(refreshes.token.token.clone());
                self.tokens_to_refresh
                    .insert(refreshes.token.token.clone(), refreshes.clone());
            }
            self.tokens_to_refresh.retain(|key, _| keys.contains(key));
            Ok(())
        } else {
            Ok(())
        }
    }

    pub async fn start_refresh_features_background_task(&self) {
        loop {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(5)) => {
                    self.refresh_features().await;
                }
            }
        }
    }

    pub async fn hydrate_new_tokens(&self) {
        let hydrations = self.get_tokens_never_refreshed();
        for hydration in hydrations {
            self.refresh_single(hydration).await;
        }
    }
    pub async fn refresh_features(&self) {
        let refreshes = self.get_tokens_due_for_refresh();
        for refresh in refreshes {
            self.refresh_single(refresh).await;
        }
    }

    pub async fn refresh_single(&self, refresh: TokenRefresh) {
        let features_result = self
            .unleash_client
            .get_client_features(ClientFeaturesRequest {
                api_key: refresh.token.token.clone(),
                etag: refresh.etag,
            })
            .await;

        match features_result {
            Ok(feature_response) => match feature_response {
                ClientFeaturesResponse::NoUpdate(tag) => {
                    debug!("No update needed. Will update last check time with {tag}");
                    self.update_last_check(&refresh.token.clone());
                }
                ClientFeaturesResponse::Updated(features, etag) => {
                    debug!("Got updated client features. Updating features with {etag:?}");
                    let key = cache_key(&refresh.token);
                    self.update_last_refresh(&refresh.token, etag);
                    self.features_cache
                        .entry(key.clone())
                        .and_modify(|existing_data| {
                            *existing_data = existing_data.clone().upsert(features.clone());
                        })
                        .or_insert_with(|| features.clone());
                    self.engine_cache
                        .entry(key.clone())
                        .and_modify(|engine| {
                            if let Some(f) = self.features_cache.get(&key) {
                                let mut new_state = EngineState::default();
                                new_state.take_state(f.clone());
                                *engine = new_state;
                            }
                        })
                        .or_insert_with(|| {
                            let mut new_state = EngineState::default();
                            new_state.take_state(features);
                            new_state
                        });
                }
            },
            Err(e) => {
                match e {
                    EdgeError::ClientFeaturesFetchError(fe) => {
                        match fe {
                            FeatureError::Retriable => {
                                warn!("Couldn't refresh features, but will retry next go")
                            }
                            FeatureError::AccessDenied => {
                                warn!("Token used to fetch features was Forbidden, will remove from list of refresh tasks");
                                self.tokens_to_refresh.remove(&refresh.token.token);
                                if !self.tokens_to_refresh.iter().any(|e| {
                                    e.value().token.environment == refresh.token.environment
                                }) {
                                    let cache_key = cache_key(&refresh.token);
                                    // No tokens left that access the environment of our current refresh. Deleting client features and engine cache
                                    self.features_cache.remove(&cache_key);
                                    self.engine_cache.remove(&cache_key);
                                }
                            }
                        }
                    }
                    _ => warn!("Couldn't refresh features: {e:?}. Will retry next pass"),
                }
            }
        }
    }

    pub fn update_last_check(&self, token: &EdgeToken) {
        if let Some(mut token) = self.tokens_to_refresh.get_mut(&token.token) {
            token.last_check = Some(Utc::now());
        }
    }

    pub fn update_last_refresh(&self, token: &EdgeToken, etag: Option<EntityTag>) {
        self.tokens_to_refresh
            .entry(token.token.clone())
            .and_modify(|token_to_refresh| {
                token_to_refresh.last_check = Some(Utc::now());
                token_to_refresh.last_refreshed = Some(Utc::now());
                token_to_refresh.etag = etag
            });
    }
}

#[cfg(test)]
mod tests {
    use actix_http::HttpService;
    use actix_http_test::{test_server, TestServer};
    use actix_service::map_config;
    use actix_web::dev::AppConfig;
    use actix_web::http::header::EntityTag;
    use actix_web::{web, App};
    use chrono::{Duration, Utc};
    use std::str::FromStr;
    use std::sync::Arc;

    use dashmap::DashMap;
    use reqwest::Url;
    use unleash_types::client_features::ClientFeatures;
    use unleash_yggdrasil::EngineState;

    use crate::tests::features_from_disk;
    use crate::tokens::cache_key;
    use crate::types::TokenType;
    use crate::types::TokenValidationStatus::Validated;
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

    #[tokio::test]
    pub async fn registering_token_for_refresh_works() {
        let unleash_client = UnleashClient::from_url(Url::parse("http://localhost:4242").unwrap());
        let features_cache = Arc::new(DashMap::default());
        let engines_cache = Arc::new(DashMap::default());

        let duration = Duration::seconds(5);
        let feature_refresher = FeatureRefresher::new(
            Arc::new(unleash_client),
            features_cache,
            engines_cache,
            duration,
            None,
        );
        let token =
            EdgeToken::try_from("*:development.abcdefghijklmnopqrstuvwxyz".to_string()).unwrap();
        let _ = feature_refresher
            .register_token_for_refresh(token, None)
            .await;

        assert_eq!(feature_refresher.tokens_to_refresh.len(), 1);
    }

    #[tokio::test]
    pub async fn registering_multiple_non_overlapping_tokens_will_keep_all() {
        let unleash_client = UnleashClient::from_url(Url::parse("http://localhost:4242").unwrap());
        let features_cache = Arc::new(DashMap::default());
        let engines_cache = Arc::new(DashMap::default());
        let duration = Duration::seconds(5);
        let feature_refresher = FeatureRefresher::new(
            Arc::new(unleash_client),
            features_cache,
            engines_cache,
            duration,
            None,
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
        let _ = feature_refresher
            .register_token_for_refresh(project_a_token, None)
            .await;
        let _ = feature_refresher
            .register_token_for_refresh(project_b_token, None)
            .await;
        let _ = feature_refresher
            .register_token_for_refresh(project_c_token, None)
            .await;

        assert_eq!(feature_refresher.tokens_to_refresh.len(), 3);
    }

    #[tokio::test]
    pub async fn registering_wildcard_project_token_only_keeps_the_wildcard() {
        let unleash_client = UnleashClient::from_url(Url::parse("http://localhost:4242").unwrap());
        let features_cache = Arc::new(DashMap::default());
        let engines_cache = Arc::new(DashMap::default());
        let duration = Duration::seconds(5);
        let feature_refresher = FeatureRefresher::new(
            Arc::new(unleash_client),
            features_cache,
            engines_cache,
            duration,
            None,
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

        let _ = feature_refresher
            .register_token_for_refresh(project_a_token, None)
            .await;
        let _ = feature_refresher
            .register_token_for_refresh(project_b_token, None)
            .await;
        let _ = feature_refresher
            .register_token_for_refresh(project_c_token, None)
            .await;
        let _ = feature_refresher
            .register_token_for_refresh(wildcard_token, None)
            .await;

        assert_eq!(feature_refresher.tokens_to_refresh.len(), 1);
        assert!(feature_refresher
            .tokens_to_refresh
            .contains_key("*:development.abcdefghijklmnopqrstuvwxyz"))
    }

    #[tokio::test]
    pub async fn registering_tokens_with_multiple_projects_overwrites_single_tokens() {
        let unleash_client = UnleashClient::from_url(Url::parse("http://localhost:4242").unwrap());
        let features_cache = Arc::new(DashMap::default());
        let engines_cache = Arc::new(DashMap::default());
        let duration = Duration::seconds(5);
        let feature_refresher = FeatureRefresher::new(
            Arc::new(unleash_client),
            features_cache,
            engines_cache,
            duration,
            None,
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

        let _ = feature_refresher
            .register_token_for_refresh(project_a_token, None)
            .await;
        let _ = feature_refresher
            .register_token_for_refresh(project_b_token, None)
            .await;
        let _ = feature_refresher
            .register_token_for_refresh(project_c_token, None)
            .await;
        let _ = feature_refresher
            .register_token_for_refresh(project_a_and_c_token, None)
            .await;

        assert_eq!(feature_refresher.tokens_to_refresh.len(), 2);
        assert!(feature_refresher
            .tokens_to_refresh
            .contains_key("[]:development.abcdefghijklmnopqrstuvwxyz"));
        assert!(feature_refresher
            .tokens_to_refresh
            .contains_key("projectb:development.abcdefghijklmnopqrstuvwxyz"));
    }

    #[tokio::test]
    pub async fn registering_a_token_that_is_already_subsumed_does_nothing() {
        let unleash_client = UnleashClient::from_url(Url::parse("http://localhost:4242").unwrap());
        let features_cache = Arc::new(DashMap::default());
        let engines_cache = Arc::new(DashMap::default());

        let duration = Duration::seconds(5);
        let feature_refresher = FeatureRefresher::new(
            Arc::new(unleash_client),
            features_cache,
            engines_cache,
            duration,
            None,
        );
        let star_token =
            EdgeToken::try_from("*:development.abcdefghijklmnopqrstuvwxyz".to_string()).unwrap();
        let project_a_token =
            EdgeToken::try_from("projecta:development.abcdefghijklmnopqrstuvwxyz".to_string())
                .unwrap();

        let _ = feature_refresher
            .register_token_for_refresh(star_token, None)
            .await;
        let _ = feature_refresher
            .register_token_for_refresh(project_a_token, None)
            .await;

        assert_eq!(feature_refresher.tokens_to_refresh.len(), 1);
        assert!(feature_refresher
            .tokens_to_refresh
            .contains_key("*:development.abcdefghijklmnopqrstuvwxyz"));
    }

    #[tokio::test]
    pub async fn simplification_only_happens_in_same_environment() {
        let unleash_client = UnleashClient::from_url(Url::parse("http://localhost:4242").unwrap());
        let features_cache = Arc::new(DashMap::default());
        let engines_cache = Arc::new(DashMap::default());

        let duration = Duration::seconds(5);
        let feature_refresher = FeatureRefresher::new(
            Arc::new(unleash_client),
            features_cache,
            engines_cache,
            duration,
            None,
        );
        let project_a_token =
            EdgeToken::try_from("projecta:development.abcdefghijklmnopqrstuvwxyz".to_string())
                .unwrap();
        let production_wildcard_token =
            EdgeToken::try_from("*:production.abcdefghijklmnopqrstuvwxyz".to_string()).unwrap();
        let _ = feature_refresher
            .register_token_for_refresh(project_a_token, None)
            .await;
        let _ = feature_refresher
            .register_token_for_refresh(production_wildcard_token, None)
            .await;
        assert_eq!(feature_refresher.tokens_to_refresh.len(), 2);
    }

    #[tokio::test]
    pub async fn is_able_to_only_fetch_for_tokens_due_to_refresh() {
        let unleash_client = UnleashClient::from_url(Url::parse("http://localhost:4242").unwrap());
        let features_cache = Arc::new(DashMap::default());
        let engines_cache = Arc::new(DashMap::default());

        let duration = Duration::seconds(5);
        let feature_refresher = FeatureRefresher::new(
            Arc::new(unleash_client),
            features_cache,
            engines_cache,
            duration,
            None,
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

    async fn client_api_test_server(
        upstream_token_cache: Arc<DashMap<String, EdgeToken>>,
        upstream_features_cache: Arc<DashMap<String, ClientFeatures>>,
        upstream_engine_cache: Arc<DashMap<String, EngineState>>,
    ) -> TestServer {
        test_server(move || {
            HttpService::new(map_config(
                App::new()
                    .app_data(web::Data::from(upstream_features_cache.clone()))
                    .app_data(web::Data::from(upstream_engine_cache.clone()))
                    .app_data(web::Data::from(upstream_token_cache.clone()))
                    .service(web::scope("/api").configure(crate::client_api::configure_client_api)),
                |_| AppConfig::default(),
            ))
            .tcp()
        })
        .await
    }
    #[tokio::test]
    pub async fn getting_403_when_refreshing_features_will_remove_token() {
        let upstream_features_cache: Arc<DashMap<String, ClientFeatures>> =
            Arc::new(DashMap::default());
        let upstream_engine_cache: Arc<DashMap<String, EngineState>> = Arc::new(DashMap::default());
        let upstream_token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());
        let server = client_api_test_server(
            upstream_token_cache,
            upstream_features_cache,
            upstream_engine_cache,
        )
        .await;
        let unleash_client = UnleashClient::new(server.url("/").as_str(), None).unwrap();
        let features_cache: Arc<DashMap<String, ClientFeatures>> = Arc::new(DashMap::default());
        let engine_cache: Arc<DashMap<String, EngineState>> = Arc::new(DashMap::default());
        let feature_refresher = FeatureRefresher::new(
            Arc::new(unleash_client),
            features_cache,
            engine_cache,
            Duration::seconds(60),
            None,
        );
        let mut token = EdgeToken::try_from("*:development.secret123".to_string()).unwrap();
        token.status = Validated;
        token.token_type = Some(TokenType::Client);
        let _ = feature_refresher
            .register_token_for_refresh(token, None)
            .await;
        assert!(!feature_refresher.tokens_to_refresh.is_empty());
        feature_refresher.refresh_features().await;
        assert!(feature_refresher.tokens_to_refresh.is_empty());
        assert!(feature_refresher.features_cache.is_empty());
        assert!(feature_refresher.engine_cache.is_empty());
    }

    #[tokio::test]
    pub async fn when_we_have_a_cache_and_token_gets_removed_caches_are_emptied() {
        let upstream_features_cache: Arc<DashMap<String, ClientFeatures>> =
            Arc::new(DashMap::default());
        let upstream_engine_cache: Arc<DashMap<String, EngineState>> = Arc::new(DashMap::default());
        let upstream_token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());
        let token_cache_to_modify = upstream_token_cache.clone();
        let mut valid_token = EdgeToken::try_from("*:development.secret123".to_string()).unwrap();
        valid_token.token_type = Some(TokenType::Client);
        valid_token.status = Validated;
        upstream_token_cache.insert(valid_token.token.clone(), valid_token.clone());
        let example_features = features_from_disk("../examples/features.json");
        let cache_key = cache_key(&valid_token);
        let mut engine_state = EngineState::default();
        engine_state.take_state(example_features.clone());
        upstream_features_cache.insert(cache_key.clone(), example_features.clone());
        upstream_engine_cache.insert(cache_key.clone(), engine_state);
        let server = client_api_test_server(
            upstream_token_cache,
            upstream_features_cache,
            upstream_engine_cache,
        )
        .await;
        let unleash_client = UnleashClient::new(server.url("/").as_str(), None).unwrap();
        let mut feature_refresher = FeatureRefresher::with_client(Arc::new(unleash_client));
        feature_refresher.refresh_interval = Duration::seconds(0);
        let _ = feature_refresher
            .register_token_for_refresh(valid_token.clone(), None)
            .await;
        assert!(!feature_refresher.tokens_to_refresh.is_empty());
        feature_refresher.refresh_features().await;
        assert!(!feature_refresher.features_cache.is_empty());
        assert!(!feature_refresher.engine_cache.is_empty());
        token_cache_to_modify.remove(&valid_token.token);
        feature_refresher.refresh_features().await;
        assert!(feature_refresher.tokens_to_refresh.is_empty());
        assert!(feature_refresher.features_cache.is_empty());
        assert!(feature_refresher.engine_cache.is_empty());
    }

    #[tokio::test]
    pub async fn removing_one_of_multiple_keys_from_same_environment_does_not_remove_feature_and_engine_caches(
    ) {
        let upstream_features_cache: Arc<DashMap<String, ClientFeatures>> =
            Arc::new(DashMap::default());
        let upstream_engine_cache: Arc<DashMap<String, EngineState>> = Arc::new(DashMap::default());
        let upstream_token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());
        let token_cache_to_modify = upstream_token_cache.clone();
        let mut dx_token = EdgeToken::try_from("dx:development.secret123".to_string()).unwrap();
        dx_token.token_type = Some(TokenType::Client);
        dx_token.status = Validated;
        upstream_token_cache.insert(dx_token.token.clone(), dx_token.clone());
        let mut eg_token = EdgeToken::try_from("eg:development.secret123".to_string()).unwrap();
        eg_token.token_type = Some(TokenType::Client);
        eg_token.status = Validated;
        upstream_token_cache.insert(eg_token.token.clone(), eg_token.clone());
        let example_features = features_from_disk("../examples/hostedexample.json");
        let cache_key = cache_key(&dx_token);
        let mut engine_state = EngineState::default();
        engine_state.take_state(example_features.clone());
        upstream_features_cache.insert(cache_key.clone(), example_features.clone());
        upstream_engine_cache.insert(cache_key.clone(), engine_state);
        let server = client_api_test_server(
            upstream_token_cache,
            upstream_features_cache,
            upstream_engine_cache,
        )
        .await;
        let unleash_client = UnleashClient::new(server.url("/").as_str(), None).unwrap();
        let mut feature_refresher = FeatureRefresher::with_client(Arc::new(unleash_client));
        feature_refresher.refresh_interval = Duration::seconds(0);
        let _ = feature_refresher
            .register_token_for_refresh(dx_token.clone(), None)
            .await;
        let _ = feature_refresher
            .register_token_for_refresh(eg_token.clone(), None)
            .await;
        assert_eq!(feature_refresher.tokens_to_refresh.len(), 2);
        assert_eq!(feature_refresher.features_cache.len(), 0);
        assert_eq!(feature_refresher.engine_cache.len(), 0);
        feature_refresher.refresh_features().await;
        assert_eq!(feature_refresher.features_cache.len(), 1);
        assert_eq!(feature_refresher.engine_cache.len(), 1);
        token_cache_to_modify.remove(&dx_token.token);
        feature_refresher.refresh_features().await;
        assert_eq!(feature_refresher.tokens_to_refresh.len(), 1);
        assert_eq!(feature_refresher.features_cache.len(), 1);
        assert_eq!(feature_refresher.engine_cache.len(), 1);
    }

    #[tokio::test]
    pub async fn fetching_two_projects_from_same_environment_should_get_features_for_both() {
        let upstream_features_cache: Arc<DashMap<String, ClientFeatures>> =
            Arc::new(DashMap::default());
        let upstream_engine_cache: Arc<DashMap<String, EngineState>> = Arc::new(DashMap::default());
        let upstream_token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());
        let mut dx_token = EdgeToken::try_from("dx:development.secret123".to_string()).unwrap();
        dx_token.token_type = Some(TokenType::Client);
        dx_token.status = Validated;
        upstream_token_cache.insert(dx_token.token.clone(), dx_token.clone());
        let mut eg_token = EdgeToken::try_from("eg:development.secret123".to_string()).unwrap();
        eg_token.token_type = Some(TokenType::Client);
        eg_token.status = Validated;
        upstream_token_cache.insert(eg_token.token.clone(), eg_token.clone());
        let example_features = features_from_disk("../examples/hostedexample.json");
        let cache_key = cache_key(&dx_token);
        upstream_features_cache.insert(cache_key.clone(), example_features.clone());
        let mut engine_state = EngineState::default();
        engine_state.take_state(example_features.clone());
        upstream_engine_cache.insert(cache_key, engine_state);
        let server = client_api_test_server(
            upstream_token_cache,
            upstream_features_cache,
            upstream_engine_cache,
        )
        .await;
        let unleash_client = UnleashClient::new(server.url("/").as_str(), None).unwrap();
        let mut feature_refresher = FeatureRefresher::with_client(Arc::new(unleash_client));
        feature_refresher.refresh_interval = Duration::seconds(0);
        let dx_features = feature_refresher
            .features_for_token(dx_token)
            .await
            .expect("No dx features");
        assert!(dx_features
            .features
            .iter()
            .all(|f| f.project == Some("dx".into())));
        assert_eq!(dx_features.features.len(), 16);
        let eg_features = feature_refresher
            .features_for_token(eg_token)
            .await
            .expect("Could not get eg features");
        assert_eq!(eg_features.features.len(), 7);
        assert!(eg_features
            .features
            .iter()
            .all(|f| f.project == Some("eg".into())));
    }

    #[tokio::test]
    pub async fn should_get_data_for_multi_project_token_even_if_we_have_data_for_one_of_the_projects(
    ) {
        let upstream_features_cache: Arc<DashMap<String, ClientFeatures>> =
            Arc::new(DashMap::default());
        let upstream_engine_cache: Arc<DashMap<String, EngineState>> = Arc::new(DashMap::default());
        let upstream_token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());
        let mut dx_token = EdgeToken::from_str("dx:development.secret123").unwrap();
        dx_token.token_type = Some(TokenType::Client);
        dx_token.status = Validated;
        upstream_token_cache.insert(dx_token.token.clone(), dx_token.clone());
        let mut multitoken = EdgeToken::from_str("[]:development.secret321").unwrap();
        multitoken.token_type = Some(TokenType::Client);
        multitoken.status = Validated;
        multitoken.projects = vec!["dx".into(), "eg".into()];
        upstream_token_cache.insert(multitoken.token.clone(), multitoken.clone());
        let mut eg_token = EdgeToken::from_str("eg:development.devsecret").unwrap();
        eg_token.token_type = Some(TokenType::Client);
        eg_token.status = Validated;
        upstream_token_cache.insert(eg_token.token.clone(), eg_token.clone());
        let example_features = features_from_disk("../examples/hostedexample.json");
        let cache_key = cache_key(&dx_token);
        upstream_features_cache.insert(cache_key.clone(), example_features.clone());
        let mut engine_state = EngineState::default();
        engine_state.take_state(example_features.clone());
        upstream_engine_cache.insert(cache_key, engine_state);
        let server = client_api_test_server(
            upstream_token_cache,
            upstream_features_cache,
            upstream_engine_cache,
        )
        .await;
        let unleash_client = UnleashClient::new(server.url("/").as_str(), None).unwrap();
        let mut feature_refresher = FeatureRefresher::with_client(Arc::new(unleash_client));
        feature_refresher.refresh_interval = Duration::seconds(0);
        let dx_features = feature_refresher
            .features_for_token(dx_token)
            .await
            .expect("No dx features found");
        assert_eq!(dx_features.features.len(), 16);
        let unleash_cloud_features = feature_refresher
            .features_for_token(multitoken)
            .await
            .expect("No multi features");
        assert_eq!(
            unleash_cloud_features
                .features
                .iter()
                .filter(|f| f.project == Some("dx".into()))
                .count(),
            16
        );
        assert_eq!(
            unleash_cloud_features
                .features
                .iter()
                .filter(|f| f.project == Some("eg".into()))
                .count(),
            7
        );
        let eg_features = feature_refresher
            .features_for_token(eg_token)
            .await
            .expect("No eg_token features");
        assert_eq!(
            eg_features
                .features
                .iter()
                .filter(|f| f.project == Some("eg".into()))
                .count(),
            7
        );
    }
}
