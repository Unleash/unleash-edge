use std::collections::HashSet;
use std::{sync::Arc, time::Duration};

pub mod delta_refresh;

use chrono::{TimeDelta, Utc};
use dashmap::DashMap;
use etag::EntityTag;
use reqwest::StatusCode;
use tracing::{debug, info, warn};
use unleash_edge_delta::cache_manager::DeltaCacheManager;
use unleash_edge_feature_cache::FeatureCache;
use unleash_edge_feature_filters::{FeatureFilterSet, filter_client_features};
use unleash_edge_http_client::{ClientMetaInformation, UnleashClient};
use unleash_edge_persistence::EdgePersistence;
use unleash_edge_types::errors::{EdgeError, FeatureError};
use unleash_edge_types::tokens::{EdgeToken, cache_key, simplify};
use unleash_edge_types::{
    ClientFeaturesRequest, ClientFeaturesResponse, EdgeResult, TokenRefresh, build,
};
use unleash_types::client_features::ClientFeatures;
use unleash_types::client_metrics::{ClientApplication, MetricsMetadata, SdkType};
use unleash_yggdrasil::{EngineState, UpdateMessage};

use crate::delta_refresh::DeltaRefresher;

pub fn frontend_token_is_covered_by_tokens(
    frontend_token: &EdgeToken,
    tokens_to_refresh: Arc<DashMap<String, TokenRefresh>>,
) -> bool {
    tokens_to_refresh.iter().any(|client_token| {
        client_token
            .token
            .same_environment_and_broader_or_equal_project_access(frontend_token)
    })
}

#[derive(Clone)]
pub enum HydratorType {
    Streaming(Arc<DeltaRefresher>),
    Polling(Arc<FeatureRefresher>),
}

impl HydratorType {
    pub async fn hydrate_new_tokens(&self) {
        match self {
            HydratorType::Streaming(delta_refresher) => delta_refresher.hydrate_new_tokens().await,
            HydratorType::Polling(feature_refresher) => {
                feature_refresher.hydrate_new_tokens().await
            }
        }
    }

    pub async fn register_token_for_refresh(&self, token: EdgeToken, etag: Option<EntityTag>) {
        match self {
            HydratorType::Streaming(delta_refresher) => {
                delta_refresher
                    .register_token_for_refresh(token, etag)
                    .await
            }
            HydratorType::Polling(feature_refresher) => {
                feature_refresher
                    .register_token_for_refresh(token, etag)
                    .await
            }
        }
    }

    pub fn tokens_to_refresh(self) -> TokenRefreshSet {
        match self {
            HydratorType::Streaming(delta_refresher) => delta_refresher.tokens_to_refresh.clone(),
            HydratorType::Polling(feature_refresher) => feature_refresher.tokens_to_refresh.clone(),
        }
    }
}

type TokenRefreshSet = Arc<DashMap<String, TokenRefresh>>;

trait TokenRefreshStatus {
    fn get_tokens_due_for_refresh(&self) -> Vec<TokenRefresh>;
    fn get_tokens_never_refreshed(&self) -> Vec<TokenRefresh>;
    fn token_is_subsumed(&self, token: &EdgeToken) -> bool;
    fn backoff(&self, token: &EdgeToken, refresh_interval: &TimeDelta);
    fn update_last_refresh(
        &self,
        token: &EdgeToken,
        etag: Option<EntityTag>,
        feature_count: usize,
        refresh_interval: &TimeDelta,
    );
    fn update_last_check(&self, token: &EdgeToken, refresh_interval: &TimeDelta);
}

impl TokenRefreshStatus for TokenRefreshSet {
    fn get_tokens_due_for_refresh(&self) -> Vec<TokenRefresh> {
        self.iter()
            .map(|e| e.value().clone())
            .filter(|token| {
                token
                    .next_refresh
                    .map(|refresh| Utc::now() > refresh)
                    .unwrap_or(true)
            })
            .collect()
    }

    fn get_tokens_never_refreshed(&self) -> Vec<TokenRefresh> {
        self.iter()
            .map(|e| e.value().clone())
            .filter(|token| token.last_refreshed.is_none() && token.last_check.is_none())
            .collect()
    }

    fn token_is_subsumed(&self, token: &EdgeToken) -> bool {
        self.iter()
            .filter(|r| r.token.environment == token.environment)
            .any(|t| t.token.subsumes(token))
    }

    fn backoff(&self, token: &EdgeToken, refresh_interval: &TimeDelta) {
        self.alter(&token.token, |_k, old_refresh| {
            old_refresh.backoff(refresh_interval)
        });
    }

    fn update_last_refresh(
        &self,
        token: &EdgeToken,
        etag: Option<EntityTag>,
        feature_count: usize,
        refresh_interval: &TimeDelta,
    ) {
        self.alter(&token.token, |_k, old_refresh| {
            old_refresh.successful_refresh(refresh_interval, etag, feature_count)
        });
    }

    fn update_last_check(&self, token: &EdgeToken, refresh_interval: &TimeDelta) {
        self.alter(&token.token, |_k, old_refresh| {
            old_refresh.successful_check(refresh_interval)
        });
    }
}

#[derive(Clone)]
pub struct FeatureRefresher {
    pub unleash_client: Arc<UnleashClient>,
    pub tokens_to_refresh: TokenRefreshSet,
    pub features_cache: Arc<FeatureCache>,
    pub delta_cache_manager: Arc<DeltaCacheManager>,
    pub engine_cache: Arc<DashMap<String, EngineState>>,
    pub refresh_interval: chrono::Duration,
    pub persistence: Option<Arc<dyn EdgePersistence>>,
    pub client_meta_information: ClientMetaInformation,
}

impl Default for FeatureRefresher {
    fn default() -> Self {
        Self {
            refresh_interval: chrono::Duration::seconds(15),
            unleash_client: Default::default(),
            tokens_to_refresh: Arc::new(DashMap::default()),
            features_cache: Arc::new(Default::default()),
            delta_cache_manager: Arc::new(DeltaCacheManager::new()),
            engine_cache: Default::default(),
            persistence: None,
            client_meta_information: Default::default(),
        }
    }
}

fn client_application_from_token_and_name(
    token: EdgeToken,
    refresh_interval: i64,
    client_meta_information: ClientMetaInformation,
) -> ClientApplication {
    ClientApplication {
        app_name: client_meta_information.app_name,
        connect_via: None,
        environment: token.environment,
        projects: Some(token.projects),
        instance_id: Some(client_meta_information.instance_id),
        connection_id: Some(client_meta_information.connection_id),
        interval: refresh_interval as u32,
        started: Utc::now(),
        strategies: vec![],
        metadata: MetricsMetadata {
            platform_name: None,
            platform_version: None,
            sdk_version: Some(format!("unleash-edge:{}", build::PKG_VERSION)),
            sdk_type: Some(SdkType::Backend),
            yggdrasil_version: None,
        },
    }
}

pub async fn start_refresh_features_background_task(refresher: Arc<FeatureRefresher>) {
    loop {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(5)) => {
                refresher.refresh_features().await;
            }
        }
    }
}

pub struct FeatureRefreshConfig {
    features_refresh_interval: chrono::Duration,
    client_meta_information: ClientMetaInformation,
}

impl FeatureRefreshConfig {
    pub fn new(
        features_refresh_interval: chrono::Duration,
        client_meta_information: ClientMetaInformation,
    ) -> Self {
        Self {
            features_refresh_interval,
            client_meta_information,
        }
    }
}

impl FeatureRefresher {
    pub fn new(
        unleash_client: Arc<UnleashClient>,
        features_cache: Arc<FeatureCache>,
        delta_cache_manager: Arc<DeltaCacheManager>,
        engines: Arc<DashMap<String, EngineState>>,
        persistence: Option<Arc<dyn EdgePersistence>>,
        config: FeatureRefreshConfig,
    ) -> Self {
        FeatureRefresher {
            unleash_client,
            tokens_to_refresh: Arc::new(DashMap::default()),
            features_cache,
            delta_cache_manager,
            engine_cache: engines,
            refresh_interval: config.features_refresh_interval,
            persistence,
            client_meta_information: config.client_meta_information,
        }
    }

    pub fn with_client(client: Arc<UnleashClient>) -> Self {
        Self {
            unleash_client: client,
            ..Default::default()
        }
    }

    /// This method no longer returns any data. Its responsibility lies in adding the token to our
    /// list of tokens to perform refreshes for, as well as calling out to hydrate tokens that we haven't seen before.
    /// Other tokens will be refreshed due to the scheduled task that refreshes tokens that haven been refreshed in ${refresh_interval} seconds
    pub async fn register_and_hydrate_token(&self, token: &EdgeToken) {
        self.register_token_for_refresh(token.clone(), None).await;
        self.hydrate_new_tokens().await;
    }

    pub fn features_for_filter(
        &self,
        token: EdgeToken,
        filters: &FeatureFilterSet,
    ) -> EdgeResult<ClientFeatures> {
        match self.get_features_by_filter(&token, filters) {
            Some(features) if self.tokens_to_refresh.token_is_subsumed(&token) => Ok(features),
            Some(_features) if !self.tokens_to_refresh.token_is_subsumed(&token) => {
                debug!("Token is not subsumed by any registered tokens. Returning error");
                Err(EdgeError::InvalidToken)
            }
            _ => {
                debug!("No features set available. Edge isn't ready");
                Err(EdgeError::InvalidToken)
            }
        }
    }

    /// Registers a token for refresh, the token will be discarded if it can be subsumed by another previously registered token
    pub async fn register_token_for_refresh(&self, token: EdgeToken, etag: Option<EntityTag>) {
        if !self.tokens_to_refresh.contains_key(&token.token) {
            self.unleash_client
                .register_as_client(
                    token.token.clone(),
                    client_application_from_token_and_name(
                        token.clone(),
                        self.refresh_interval.num_seconds(),
                        self.client_meta_information.clone(),
                    ),
                )
                .await
                .unwrap_or_default();
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
        }
    }

    pub async fn hydrate_new_tokens(&self) {
        for hydration in self.tokens_to_refresh.get_tokens_never_refreshed() {
            self.refresh_single(hydration).await;
        }
    }

    pub async fn refresh_features(&self) {
        for refresh in self.tokens_to_refresh.get_tokens_due_for_refresh() {
            self.refresh_single(refresh).await;
        }
    }

    async fn handle_client_features_updated(
        &self,
        refresh_token: &EdgeToken,
        features: ClientFeatures,
        etag: Option<EntityTag>,
    ) {
        debug!("Got updated client features. Updating features with {etag:?}");
        let key = cache_key(refresh_token);
        self.tokens_to_refresh.update_last_refresh(
            refresh_token,
            etag,
            features.features.len(),
            &self.refresh_interval,
        );
        self.features_cache
            .modify(key.clone(), refresh_token, features.clone());
        self.engine_cache
            .entry(key.clone())
            .and_modify(|engine| {
                if let Some(f) = self.features_cache.get(&key) {
                    let mut new_state = EngineState::default();
                    let warnings = new_state.take_state(UpdateMessage::FullResponse(f.clone()));
                    if let Some(warnings) = warnings {
                        warn!("The following toggle failed to compile and will be defaulted to off: {warnings:?}");
                    };
                    *engine = new_state;

                }
            })
            .or_insert_with(|| {
                let mut new_state = EngineState::default();

                let warnings = new_state.take_state(UpdateMessage::FullResponse(features));
                if let Some(warnings) = warnings {
                    warn!("The following toggle failed to compile and will be defaulted to off: {warnings:?}");
                };
                new_state
            });
    }

    pub async fn refresh_single(&self, refresh: TokenRefresh) {
        let features_result = self
            .unleash_client
            .get_client_features(ClientFeaturesRequest {
                api_key: refresh.token.token.clone(),
                etag: refresh.etag.clone(),
                interval: Some(self.refresh_interval.num_milliseconds()),
            })
            .await;
        match features_result {
            Ok(feature_response) => match feature_response {
                ClientFeaturesResponse::NoUpdate(tag) => {
                    debug!("No update needed. Will update last check time with {tag}");
                    self.tokens_to_refresh
                        .update_last_check(&refresh.token.clone(), &self.refresh_interval);
                }
                ClientFeaturesResponse::Updated(features, etag) => {
                    self.handle_client_features_updated(&refresh.token, features, etag)
                        .await;
                }
            },
            Err(e) => {
                match e {
                    EdgeError::ClientFeaturesFetchError(fe) => {
                        match fe {
                            FeatureError::Retriable(status_code) => match status_code {
                                StatusCode::INTERNAL_SERVER_ERROR
                                | StatusCode::BAD_GATEWAY
                                | StatusCode::SERVICE_UNAVAILABLE
                                | StatusCode::GATEWAY_TIMEOUT => {
                                    info!(
                                        "Upstream is having some problems, increasing my waiting period"
                                    );
                                    self.tokens_to_refresh
                                        .backoff(&refresh.token, &self.refresh_interval);
                                }
                                StatusCode::TOO_MANY_REQUESTS => {
                                    info!("Got told that upstream is receiving too many requests");
                                    self.tokens_to_refresh
                                        .backoff(&refresh.token, &self.refresh_interval);
                                }
                                _ => {
                                    info!("Couldn't refresh features, but will retry next go")
                                }
                            },
                            FeatureError::AccessDenied => {
                                info!(
                                    "Token used to fetch features was Forbidden, will remove from list of refresh tasks"
                                );
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
                            FeatureError::NotFound => {
                                info!(
                                    "Had a bad URL when trying to fetch features. Increasing waiting period for the token before trying again"
                                );
                                self.tokens_to_refresh
                                    .backoff(&refresh.token, &self.refresh_interval);
                            }
                        }
                    }
                    EdgeError::ClientCacheError => {
                        info!("Couldn't refresh features, but will retry next go")
                    }
                    _ => info!("Couldn't refresh features: {e:?}. Will retry next pass"),
                }
            }
        }
    }

    fn get_features_by_filter(
        &self,
        token: &EdgeToken,
        filters: &FeatureFilterSet,
    ) -> Option<ClientFeatures> {
        self.features_cache
            .get(&cache_key(token))
            .map(|client_features| filter_client_features(&client_features, filters))
    }
}

#[cfg(test)]
mod tests {
    use crate::TokenRefreshStatus;

    use super::FeatureRefresher;
    use chrono::{Duration, Utc};
    use dashmap::DashMap;
    use etag::EntityTag;
    use reqwest::Url;
    use std::sync::Arc;
    use unleash_edge_feature_cache::FeatureCache;
    use unleash_edge_http_client::{
        ClientMetaInformation, HttpClientArgs, UnleashClient, new_reqwest_client,
    };
    use unleash_edge_types::TokenRefresh;
    use unleash_edge_types::tokens::EdgeToken;

    fn create_test_client() -> UnleashClient {
        let http_client = new_reqwest_client(HttpClientArgs {
            client_meta_information: ClientMetaInformation::test_config(),
            ..Default::default()
        })
        .expect("Failed to create client");

        UnleashClient::from_url_with_backing_client(
            Url::parse("http://localhost:4242").unwrap(),
            "Authorization".to_string(),
            http_client,
            ClientMetaInformation::test_config(),
        )
    }

    #[tokio::test]
    pub async fn registering_token_for_refresh_works() {
        let unleash_client = create_test_client();
        let features_cache = Arc::new(FeatureCache::default());
        let engine_cache = Arc::new(DashMap::default());

        let duration = Duration::seconds(5);
        let feature_refresher = FeatureRefresher {
            unleash_client: Arc::new(unleash_client),
            features_cache,
            engine_cache,
            refresh_interval: duration,
            ..Default::default()
        };
        let token =
            EdgeToken::try_from("*:development.abcdefghijklmnopqrstuvwxyz".to_string()).unwrap();
        feature_refresher
            .register_token_for_refresh(token, None)
            .await;

        assert_eq!(feature_refresher.tokens_to_refresh.len(), 1);
    }

    #[tokio::test]
    pub async fn registering_multiple_tokens_with_same_environment_reduces_tokens_to_valid_minimal_set()
     {
        let unleash_client = create_test_client();
        let features_cache = Arc::new(FeatureCache::default());
        let engine_cache = Arc::new(DashMap::default());

        let duration = Duration::seconds(5);
        let feature_refresher = FeatureRefresher {
            unleash_client: Arc::new(unleash_client),
            features_cache,
            engine_cache,
            refresh_interval: duration,
            ..Default::default()
        };
        let token1 =
            EdgeToken::try_from("*:development.abcdefghijklmnopqrstuvwxyz".to_string()).unwrap();
        let token2 =
            EdgeToken::try_from("*:development.zyxwvutsrqponmlkjihgfedcba".to_string()).unwrap();
        feature_refresher
            .register_token_for_refresh(token1, None)
            .await;
        feature_refresher
            .register_token_for_refresh(token2, None)
            .await;

        assert_eq!(feature_refresher.tokens_to_refresh.len(), 1);
    }

    #[tokio::test]
    pub async fn registering_multiple_non_overlapping_tokens_will_keep_all() {
        let unleash_client = create_test_client();
        let features_cache = Arc::new(FeatureCache::default());
        let engine_cache = Arc::new(DashMap::default());
        let duration = Duration::seconds(5);
        let feature_refresher = FeatureRefresher {
            unleash_client: Arc::new(unleash_client),
            features_cache,
            engine_cache,
            refresh_interval: duration,
            ..Default::default()
        };
        let project_a_token =
            EdgeToken::try_from("projecta:development.abcdefghijklmnopqrstuvwxyz".to_string())
                .unwrap();
        let project_b_token =
            EdgeToken::try_from("projectb:development.abcdefghijklmnopqrstuvwxyz".to_string())
                .unwrap();
        let project_c_token =
            EdgeToken::try_from("projectc:development.abcdefghijklmnopqrstuvwxyz".to_string())
                .unwrap();
        feature_refresher
            .register_token_for_refresh(project_a_token, None)
            .await;
        feature_refresher
            .register_token_for_refresh(project_b_token, None)
            .await;
        feature_refresher
            .register_token_for_refresh(project_c_token, None)
            .await;

        assert_eq!(feature_refresher.tokens_to_refresh.len(), 3);
    }

    #[tokio::test]
    pub async fn registering_wildcard_project_token_only_keeps_the_wildcard() {
        let unleash_client = create_test_client();
        let features_cache = Arc::new(FeatureCache::default());
        let engine_cache = Arc::new(DashMap::default());
        let duration = Duration::seconds(5);
        let feature_refresher = FeatureRefresher {
            unleash_client: Arc::new(unleash_client),
            features_cache,
            engine_cache,
            refresh_interval: duration,
            ..Default::default()
        };
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

        feature_refresher
            .register_token_for_refresh(project_a_token, None)
            .await;
        feature_refresher
            .register_token_for_refresh(project_b_token, None)
            .await;
        feature_refresher
            .register_token_for_refresh(project_c_token, None)
            .await;
        feature_refresher
            .register_token_for_refresh(wildcard_token, None)
            .await;

        assert_eq!(feature_refresher.tokens_to_refresh.len(), 1);
        assert!(
            feature_refresher
                .tokens_to_refresh
                .contains_key("*:development.abcdefghijklmnopqrstuvwxyz")
        )
    }

    #[tokio::test]
    pub async fn registering_tokens_with_multiple_projects_overwrites_single_tokens() {
        let unleash_client = create_test_client();
        let features_cache = Arc::new(FeatureCache::default());
        let engine_cache = Arc::new(DashMap::default());
        let duration = Duration::seconds(5);
        let feature_refresher = FeatureRefresher {
            unleash_client: Arc::new(unleash_client),
            features_cache,
            engine_cache,
            refresh_interval: duration,
            ..Default::default()
        };
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

        feature_refresher
            .register_token_for_refresh(project_a_token, None)
            .await;
        feature_refresher
            .register_token_for_refresh(project_b_token, None)
            .await;
        feature_refresher
            .register_token_for_refresh(project_c_token, None)
            .await;
        feature_refresher
            .register_token_for_refresh(project_a_and_c_token, None)
            .await;

        assert_eq!(feature_refresher.tokens_to_refresh.len(), 2);
        assert!(
            feature_refresher
                .tokens_to_refresh
                .contains_key("[]:development.abcdefghijklmnopqrstuvwxyz")
        );
        assert!(
            feature_refresher
                .tokens_to_refresh
                .contains_key("projectb:development.abcdefghijklmnopqrstuvwxyz")
        );
    }

    #[tokio::test]
    pub async fn registering_a_token_that_is_already_subsumed_does_nothing() {
        let unleash_client = create_test_client();
        let features_cache = Arc::new(FeatureCache::default());
        let engine_cache = Arc::new(DashMap::default());

        let duration = Duration::seconds(5);
        let feature_refresher = FeatureRefresher {
            unleash_client: Arc::new(unleash_client),
            features_cache,
            engine_cache,
            refresh_interval: duration,
            ..Default::default()
        };
        let star_token =
            EdgeToken::try_from("*:development.abcdefghijklmnopqrstuvwxyz".to_string()).unwrap();
        let project_a_token =
            EdgeToken::try_from("projecta:development.abcdefghijklmnopqrstuvwxyz".to_string())
                .unwrap();

        feature_refresher
            .register_token_for_refresh(star_token, None)
            .await;
        feature_refresher
            .register_token_for_refresh(project_a_token, None)
            .await;

        assert_eq!(feature_refresher.tokens_to_refresh.len(), 1);
        assert!(
            feature_refresher
                .tokens_to_refresh
                .contains_key("*:development.abcdefghijklmnopqrstuvwxyz")
        );
    }

    #[tokio::test]
    pub async fn simplification_only_happens_in_same_environment() {
        let unleash_client = create_test_client();
        let features_cache = Arc::new(FeatureCache::default());
        let engine_cache = Arc::new(DashMap::default());

        let duration = Duration::seconds(5);
        let feature_refresher = FeatureRefresher {
            unleash_client: Arc::new(unleash_client),
            features_cache,
            engine_cache,
            refresh_interval: duration,
            ..Default::default()
        };
        let project_a_token =
            EdgeToken::try_from("projecta:development.abcdefghijklmnopqrstuvwxyz".to_string())
                .unwrap();
        let production_wildcard_token =
            EdgeToken::try_from("*:production.abcdefghijklmnopqrstuvwxyz".to_string()).unwrap();
        feature_refresher
            .register_token_for_refresh(project_a_token, None)
            .await;
        feature_refresher
            .register_token_for_refresh(production_wildcard_token, None)
            .await;
        assert_eq!(feature_refresher.tokens_to_refresh.len(), 2);
    }

    #[tokio::test]
    pub async fn is_able_to_only_fetch_for_tokens_due_to_refresh() {
        let unleash_client = create_test_client();
        let features_cache = Arc::new(FeatureCache::default());
        let engine_cache = Arc::new(DashMap::default());
        let tokens_to_refresh = Arc::new(DashMap::default());

        let duration = Duration::seconds(5);
        let feature_refresher = FeatureRefresher {
            unleash_client: Arc::new(unleash_client),
            features_cache,
            engine_cache,
            tokens_to_refresh: tokens_to_refresh.clone(),
            refresh_interval: duration,
            ..Default::default()
        };
        let no_etag_due_for_refresh_token =
            EdgeToken::try_from("projecta:development.no_etag_due_for_refresh_token".to_string())
                .unwrap();
        let no_etag_so_is_due_for_refresh = TokenRefresh {
            token: no_etag_due_for_refresh_token,
            etag: None,
            next_refresh: None,
            last_refreshed: None,
            last_check: None,
            failure_count: 0,
            last_feature_count: None,
        };
        let etag_and_last_refreshed_token =
            EdgeToken::try_from("projectb:development.etag_and_last_refreshed_token".to_string())
                .unwrap();
        let etag_and_last_refreshed_less_than_duration_ago = TokenRefresh {
            token: etag_and_last_refreshed_token,
            etag: Some(EntityTag::new(true, "abcde")),
            next_refresh: Some(Utc::now() + Duration::seconds(10)),
            last_refreshed: Some(Utc::now()),
            last_check: Some(Utc::now()),
            failure_count: 0,
            last_feature_count: None,
        };
        let etag_but_old_token =
            EdgeToken::try_from("projectb:development.etag_but_old_token".to_string()).unwrap();

        let ten_seconds_ago = Utc::now() - Duration::seconds(10);
        let etag_but_last_refreshed_ten_seconds_ago = TokenRefresh {
            token: etag_but_old_token,
            etag: Some(EntityTag::new(true, "abcde")),
            next_refresh: None,
            last_refreshed: Some(ten_seconds_ago),
            last_check: Some(ten_seconds_ago),
            failure_count: 0,
            last_feature_count: None,
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
        let tokens_to_refresh = tokens_to_refresh.get_tokens_due_for_refresh();
        assert_eq!(tokens_to_refresh.len(), 2);
        assert!(tokens_to_refresh.contains(&etag_but_last_refreshed_ten_seconds_ago));
        assert!(tokens_to_refresh.contains(&no_etag_so_is_due_for_refresh));
    }
}
