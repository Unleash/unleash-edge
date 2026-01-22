use std::collections::HashSet;
use std::sync::LazyLock;
use std::{sync::Arc, time::Duration};

pub mod delta_refresh;

use crate::delta_refresh::DeltaRefresher;
use chrono::{TimeDelta, Utc};
use dashmap::DashMap;
use etag::EntityTag;
use prometheus::{IntGaugeVec, register_int_gauge_vec};
use reqwest::StatusCode;
use tokio::sync::watch::Receiver;
use tracing::{debug, info, instrument, warn};
use unleash_edge_delta::cache_manager::DeltaCacheManager;
use unleash_edge_feature_cache::FeatureCache;
use unleash_edge_feature_filters::{FeatureFilterSet, filter_client_features};
use unleash_edge_http_client::{ClientMetaInformation, UnleashClient};
use unleash_edge_persistence::EdgePersistence;
use unleash_edge_types::errors::{EdgeError, FeatureError};
use unleash_edge_types::metrics::instance_data::EdgeInstanceData;
use unleash_edge_types::tokens::{EdgeToken, cache_key, simplify};
use unleash_edge_types::{
    ClientFeaturesRequest, ClientFeaturesResponse, EdgeResult, RefreshState, TokenRefresh, build,
};
use unleash_types::client_features::ClientFeatures;
use unleash_types::client_metrics::{ClientApplication, MetricsMetadata, SdkType};
use unleash_yggdrasil::{EngineState, UpdateMessage};

static POLLING_REVISION_ID: LazyLock<IntGaugeVec> = LazyLock::new(|| {
    register_int_gauge_vec!(
        "polling_revision_id",
        "Revision ID for polling fetcher",
        &["environment", "projects"]
    )
    .unwrap()
});

static POLLING_LAST_UPDATE: LazyLock<IntGaugeVec> = LazyLock::new(|| {
    register_int_gauge_vec!(
        "polling_last_update",
        "Timestamp of last update for polling fetcher",
        &["environment", "projects"]
    )
    .unwrap()
});

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
        revision_id: Option<usize>,
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
        revision_id: Option<usize>,
        refresh_interval: &TimeDelta,
    ) {
        self.alter(&token.token, |_k, old_refresh| {
            old_refresh.successful_refresh(refresh_interval, etag, feature_count, revision_id)
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
    pub edge_instance_data: Arc<EdgeInstanceData>,
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
        instance_id: Some(client_meta_information.instance_id.to_string()),
        connection_id: Some(client_meta_information.connection_id.to_string()),
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

pub fn features_for_filter(
    tokens_to_refresh: &TokenRefreshSet,
    features_cache: &FeatureCache,
    token: EdgeToken,
    filters: &FeatureFilterSet,
) -> EdgeResult<ClientFeatures> {
    match get_features_by_filter(features_cache, &token, filters) {
        Some(features) if tokens_to_refresh.token_is_subsumed(&token) => Ok(features),
        Some(_features) if !tokens_to_refresh.token_is_subsumed(&token) => {
            debug!("Token is not subsumed by any registered tokens. Returning error");
            Err(EdgeError::InvalidToken)
        }
        _ => {
            debug!("No features set available. Edge isn't ready");
            Err(EdgeError::InvalidToken)
        }
    }
}

fn get_features_by_filter(
    features_cache: &FeatureCache,
    token: &EdgeToken,
    filters: &FeatureFilterSet,
) -> Option<ClientFeatures> {
    features_cache
        .get(&cache_key(token))
        .map(|client_features| filter_client_features(&client_features, filters))
}

pub async fn start_refresh_features_background_task(
    refresher: Arc<FeatureRefresher>,
    refresh_state_rx: Receiver<RefreshState>,
) {
    let mut rx = refresh_state_rx;
    loop {
        if *rx.borrow_and_update() == RefreshState::Paused {
            debug!("Refresh paused, skipping this cycle");
        } else {
            refresher.refresh_features().await;
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

#[derive(Clone)]
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
        edge_instance_data: Arc<EdgeInstanceData>,
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
            edge_instance_data,
        }
    }

    /// This method no longer returns any data. Its responsibility lies in adding the token to our
    /// list of tokens to perform refreshes for, as well as calling out to hydrate tokens that we haven't seen before.
    /// Other tokens will be refreshed due to the scheduled task that refreshes tokens that haven been refreshed in ${refresh_interval} seconds
    pub async fn register_and_hydrate_token(&self, token: &EdgeToken) {
        self.register_token_for_refresh(token.clone(), None).await;
        self.hydrate_new_tokens().await;
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
        let revision_id = features.meta.as_ref().and_then(|m| m.revision_id);
        if let (Some(revision_id), Some(env)) = (revision_id, refresh_token.environment.as_ref()) {
            POLLING_REVISION_ID
                .with_label_values(&[env, &refresh_token.projects.join(",")])
                .set(revision_id as i64);
            self.edge_instance_data.observe_api_key_refresh(
                env.clone(),
                refresh_token.projects.clone(),
                revision_id,
                Utc::now(),
            );
        }
        POLLING_LAST_UPDATE
            .with_label_values(&[
                &refresh_token.environment.clone().unwrap_or("*".to_string()),
                &refresh_token.projects.join(","),
            ])
            .set(Utc::now().timestamp());
        self.tokens_to_refresh.update_last_refresh(
            refresh_token,
            etag,
            features.features.len(),
            revision_id,
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

    #[instrument(skip(self))]
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
}

#[cfg(test)]
mod tests {
    use crate::TokenRefreshStatus;

    use super::*;
    use axum::extract::FromRef;
    use chrono::{Duration, Utc};
    use dashmap::DashMap;
    use etag::EntityTag;
    use reqwest::Url;
    use std::fs;
    use std::io::BufReader;
    use std::path::PathBuf;
    use std::sync::Arc;
    use ulid::Ulid;
    use unleash_edge_cli::AuthHeaders;
    use unleash_edge_delta::cache_manager::DeltaCacheManager;
    use unleash_edge_feature_cache::FeatureCache;
    use unleash_edge_http_client::{
        ClientMetaInformation, HttpClientArgs, UnleashClient, new_reqwest_client,
    };
    use unleash_edge_types::tokens::EdgeToken;

    use axum::Router;
    use axum_test::TestServer;
    use pretty_assertions::assert_eq;
    use std::str::FromStr;
    use unleash_edge_feature_cache::update_projects_from_feature_update;

    use unleash_edge_appstate::edge_token_extractor::AuthState;
    use unleash_edge_client_api::features::{FeatureState, features_router_for};
    use unleash_edge_edge_api::{EdgeApiState, edge_api_router_for};
    use unleash_edge_types::TokenValidationStatus::Validated;
    use unleash_edge_types::metrics::instance_data::Hosting;
    use unleash_edge_types::tokens::cache_key;
    use unleash_edge_types::{EngineCache, TokenCache, TokenRefresh, TokenType};
    use unleash_types::client_features::{ClientFeature, ClientFeatures};
    use unleash_yggdrasil::{EngineState, UpdateMessage};

    impl Default for FeatureRefresher {
        fn default() -> Self {
            Self {
                refresh_interval: chrono::Duration::seconds(15),
                unleash_client: Arc::new(create_test_client(TestClientOptions::default())),
                tokens_to_refresh: Arc::new(DashMap::default()),
                features_cache: Arc::new(Default::default()),
                delta_cache_manager: Arc::new(DeltaCacheManager::new()),
                engine_cache: Default::default(),
                persistence: None,
                client_meta_information: ClientMetaInformation {
                    app_name: "test-application".to_string(),
                    instance_id: Ulid::new(),
                    connection_id: Ulid::new(),
                },
                edge_instance_data: Arc::new(edge_instance_data_for_feature_refresher_test()),
            }
        }
    }

    fn edge_instance_data_for_feature_refresher_test() -> EdgeInstanceData {
        EdgeInstanceData {
            identifier: Ulid::new().to_string(),
            app_name: "app_testing".to_string(),
            hosting: Some(Hosting::SelfHosted),
            region: None,
            edge_version: "".to_string(),
            process_metrics: None,
            started: Default::default(),
            traffic: Default::default(),
            latency_upstream: Default::default(),
            requests_since_last_report: Default::default(),
            connected_streaming_clients: 0,
            connected_edges: vec![],
            connection_consumption_since_last_report: Default::default(),
            request_consumption_since_last_report: Default::default(),
            edge_api_key_revision_ids: Default::default(),
        }
    }

    #[derive(Default)]
    struct TestClientOptions {
        client_url: Option<Url>,
    }

    fn create_test_client(TestClientOptions { client_url }: TestClientOptions) -> UnleashClient {
        let client_meta_information = ClientMetaInformation {
            app_name: "unleash-edge-test".into(),
            instance_id: Ulid::new(),
            connection_id: Ulid::new(),
        };

        UnleashClient::from_url_with_backing_client(
            client_url.unwrap_or_else(|| Url::parse("http://localhost:4242").unwrap()),
            "Authorization".to_string(),
            new_reqwest_client(HttpClientArgs {
                skip_ssl_verification: false,
                client_identity: None,
                upstream_certificate_file: None,
                connect_timeout: Duration::seconds(5),
                socket_timeout: Duration::seconds(5),
                keep_alive_timeout: Duration::seconds(15),
                client_meta_information: client_meta_information.clone(),
            })
            .expect("Failed to create client"),
            client_meta_information,
        )
    }

    #[tokio::test]
    pub async fn registering_token_for_refresh_works() {
        let unleash_client = create_test_client(TestClientOptions::default());
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
        let unleash_client = create_test_client(TestClientOptions::default());
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
        let unleash_client = create_test_client(TestClientOptions::default());
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
        let unleash_client = create_test_client(TestClientOptions::default());
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
        let unleash_client = create_test_client(TestClientOptions::default());
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
        let unleash_client = create_test_client(TestClientOptions::default());
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
        let unleash_client = create_test_client(TestClientOptions::default());
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
        let unleash_client = create_test_client(TestClientOptions::default());
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
            revision_id: None,
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
            revision_id: None,
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
            revision_id: None,
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

    #[derive(Clone)]
    struct TestState {
        upstream_token_cache: Arc<TokenCache>,
        upstream_features_cache: Arc<FeatureCache>,
        auth: AuthState,
    }

    impl FromRef<TestState> for AuthState {
        fn from_ref(s: &TestState) -> Self {
            s.auth.clone()
        }
    }

    impl FromRef<TestState> for FeatureState {
        fn from_ref(s: &TestState) -> Self {
            FeatureState {
                tokens_to_refresh: None, //this is cheating, this will skip token subsumption but it's good enough for tests
                features_cache: s.upstream_features_cache.clone(),
                token_cache: s.upstream_token_cache.clone(),
            }
        }
    }

    impl FromRef<TestState> for EdgeApiState {
        fn from_ref(input: &TestState) -> Self {
            EdgeApiState {
                token_cache: input.upstream_token_cache.clone(),
                token_validator: Arc::new(None),
            }
        }
    }

    async fn client_api_test_server(
        upstream_token_cache: Arc<TokenCache>,
        upstream_features_cache: Arc<FeatureCache>,
    ) -> TestServer {
        let test_state = TestState {
            upstream_features_cache,
            upstream_token_cache: upstream_token_cache.clone(),
            auth: AuthState {
                auth_headers: AuthHeaders::default(),
                token_cache: upstream_token_cache,
            },
        };

        let router = Router::new()
            .nest("/api/client", features_router_for::<TestState>())
            .nest("/edge", edge_api_router_for::<TestState>())
            .with_state(test_state);
        TestServer::builder()
            .http_transport()
            .build(router)
            .expect("Failed to build client api test server")
    }

    #[tokio::test]
    pub async fn getting_403_when_refreshing_features_will_remove_token() {
        let upstream_features_cache: Arc<FeatureCache> = Arc::new(FeatureCache::default());
        let upstream_token_cache: Arc<TokenCache> = Arc::new(DashMap::default());
        let server = client_api_test_server(upstream_token_cache, upstream_features_cache).await;

        let unleash_client = create_test_client(TestClientOptions {
            client_url: Some(server.server_url("/").unwrap()),
        });
        let features_cache: Arc<FeatureCache> = Arc::new(FeatureCache::default());
        let engine_cache: Arc<EngineCache> = Arc::new(DashMap::default());
        let feature_refresher = FeatureRefresher {
            unleash_client: Arc::new(unleash_client),
            features_cache,
            engine_cache,
            refresh_interval: Duration::seconds(60),
            ..Default::default()
        };
        let mut token = EdgeToken::try_from("*:development.secret123".to_string()).unwrap();
        token.status = Validated;
        token.token_type = Some(TokenType::Backend);
        feature_refresher
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
        let upstream_features_cache: Arc<FeatureCache> = Arc::new(FeatureCache::default());
        let upstream_engine_cache: Arc<DashMap<String, EngineState>> = Arc::new(DashMap::default());
        let upstream_token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());
        let token_cache_to_modify = upstream_token_cache.clone();
        let mut valid_token = EdgeToken::try_from("*:development.secret123".to_string()).unwrap();
        valid_token.token_type = Some(TokenType::Backend);
        valid_token.status = Validated;
        upstream_token_cache.insert(valid_token.token.clone(), valid_token.clone());
        let example_features = features_from_disk("../../../examples/features.json");
        let cache_key = cache_key(&valid_token);
        let mut engine_state = EngineState::default();
        let warnings =
            engine_state.take_state(UpdateMessage::FullResponse(example_features.clone()));
        upstream_features_cache.insert(cache_key.clone(), example_features.clone());
        upstream_engine_cache.insert(cache_key.clone(), engine_state);
        let server = client_api_test_server(upstream_token_cache, upstream_features_cache).await;
        let unleash_client = create_test_client(TestClientOptions {
            client_url: Some(server.server_url("/").unwrap()),
        });
        let features_cache: Arc<FeatureCache> = Arc::new(FeatureCache::default());
        let engine_cache: Arc<EngineCache> = Arc::new(DashMap::default());
        let mut feature_refresher = FeatureRefresher {
            unleash_client: Arc::new(unleash_client),
            features_cache,
            engine_cache,
            refresh_interval: Duration::seconds(60),
            ..Default::default()
        };
        feature_refresher.refresh_interval = Duration::seconds(0);
        feature_refresher
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
        assert!(warnings.is_none());
    }

    #[tokio::test]
    pub async fn removing_one_of_multiple_keys_from_same_environment_does_not_remove_feature_and_engine_caches()
     {
        let upstream_features_cache: Arc<FeatureCache> = Arc::new(FeatureCache::default());
        let upstream_engine_cache: Arc<DashMap<String, EngineState>> = Arc::new(DashMap::default());
        let upstream_token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());
        let token_cache_to_modify = upstream_token_cache.clone();
        let mut dx_token = EdgeToken::try_from("dx:development.secret123".to_string()).unwrap();
        dx_token.token_type = Some(TokenType::Backend);
        dx_token.status = Validated;
        upstream_token_cache.insert(dx_token.token.clone(), dx_token.clone());
        let mut eg_token = EdgeToken::try_from("eg:development.secret123".to_string()).unwrap();
        eg_token.token_type = Some(TokenType::Backend);
        eg_token.status = Validated;
        upstream_token_cache.insert(eg_token.token.clone(), eg_token.clone());
        let example_features = features_from_disk("../../../examples/hostedexample.json");
        let cache_key = cache_key(&dx_token);
        let mut engine_state = EngineState::default();
        let warnings =
            engine_state.take_state(UpdateMessage::FullResponse(example_features.clone()));
        upstream_features_cache.insert(cache_key.clone(), example_features.clone());
        upstream_engine_cache.insert(cache_key.clone(), engine_state);
        let server = client_api_test_server(upstream_token_cache, upstream_features_cache).await;
        let unleash_client = create_test_client(TestClientOptions {
            client_url: Some(server.server_url("/").unwrap()),
        });
        let features_cache: Arc<FeatureCache> = Arc::new(FeatureCache::default());
        let engine_cache: Arc<EngineCache> = Arc::new(DashMap::default());
        let mut feature_refresher = FeatureRefresher {
            unleash_client: Arc::new(unleash_client),
            features_cache,
            engine_cache,
            refresh_interval: Duration::seconds(60),
            ..Default::default()
        };
        feature_refresher.refresh_interval = Duration::seconds(0);
        feature_refresher
            .register_token_for_refresh(dx_token.clone(), None)
            .await;
        feature_refresher
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
        assert!(warnings.is_none());
    }

    #[test]
    fn front_end_token_is_properly_covered_by_current_tokens() {
        let fe_token = EdgeToken {
            projects: vec!["a".into(), "b".into()],
            environment: Some("development".into()),
            ..Default::default()
        };

        let wildcard_token = EdgeToken {
            projects: vec!["*".into()],
            environment: Some("development".into()),
            ..Default::default()
        };

        let current_tokens = DashMap::new();
        let token_refresh = TokenRefresh {
            token: wildcard_token.clone(),
            etag: None,
            next_refresh: None,
            last_refreshed: None,
            last_check: None,
            failure_count: 0,
            last_feature_count: None,
            revision_id: None,
        };

        current_tokens.insert(wildcard_token.token, token_refresh);

        let current_tokens_arc = Arc::new(current_tokens);
        assert!(frontend_token_is_covered_by_tokens(
            &fe_token,
            current_tokens_arc
        ));
    }

    #[tokio::test]
    async fn refetching_data_when_feature_is_archived_should_remove_archived_feature() {
        let upstream_features_cache: Arc<FeatureCache> = Arc::new(FeatureCache::default());
        let upstream_engine_cache: Arc<DashMap<String, EngineState>> = Arc::new(DashMap::default());
        let upstream_token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());
        let mut eg_token = EdgeToken::from_str("eg:development.devsecret").unwrap();
        eg_token.token_type = Some(TokenType::Backend);
        eg_token.status = Validated;
        upstream_token_cache.insert(eg_token.token.clone(), eg_token.clone());
        let example_features = features_from_disk("../../../examples/hostedexample.json");
        let cache_key = cache_key(&eg_token);
        upstream_features_cache.insert(cache_key.clone(), example_features.clone());
        let mut engine_state = EngineState::default();
        let warnings =
            engine_state.take_state(UpdateMessage::FullResponse(example_features.clone()));
        upstream_engine_cache.insert(cache_key.clone(), engine_state);
        let server =
            client_api_test_server(upstream_token_cache, upstream_features_cache.clone()).await;
        let features_cache: Arc<FeatureCache> = Arc::new(FeatureCache::default());
        let unleash_client = create_test_client(TestClientOptions {
            client_url: Some(server.server_url("/").unwrap()),
        });
        let feature_refresher = FeatureRefresher {
            unleash_client: Arc::new(unleash_client),
            features_cache: features_cache.clone(),
            refresh_interval: Duration::seconds(0),
            ..Default::default()
        };

        let _ = feature_refresher
            .register_and_hydrate_token(&eg_token)
            .await;

        // Now, let's say that all features are archived in upstream
        let empty_features = features_from_disk("../../../examples/empty-features.json");
        upstream_features_cache.insert(cache_key.clone(), empty_features);

        feature_refresher.refresh_features().await;
        // Since our response was empty, our theory is that there should be no features here now.
        assert!(
            !features_cache
                .get(&cache_key)
                .unwrap()
                .features
                .iter()
                .any(|f| f.project == Some("eg".into()))
        );
        assert!(warnings.is_none());
    }

    #[test]
    pub fn an_update_with_one_feature_removed_from_one_project_removes_the_feature_from_the_feature_list()
     {
        let features = features_from_disk("../../../examples/hostedexample.json").features;
        let mut dx_data: Vec<ClientFeature> =
            features_from_disk("../../../examples/hostedexample.json")
                .features
                .iter()
                .filter(|f| f.project == Some("dx".into()))
                .cloned()
                .collect();
        dx_data.remove(0);
        let mut token = EdgeToken::from_str("[]:development.somesecret").unwrap();
        token.status = Validated;
        token.projects = vec![String::from("dx")];

        let updated = update_projects_from_feature_update(&token, &features, &dx_data);
        assert_ne!(
            features
                .iter()
                .filter(|p| p.project == Some("dx".into()))
                .count(),
            updated
                .iter()
                .filter(|p| p.project == Some("dx".into()))
                .count()
        );
        assert_eq!(
            features
                .iter()
                .filter(|p| p.project == Some("eg".into()))
                .count(),
            updated
                .iter()
                .filter(|p| p.project == Some("eg".into()))
                .count()
        );
    }

    #[test]
    pub fn project_state_from_update_should_overwrite_project_state_in_known_state() {
        let features = features_from_disk("../../../examples/hostedexample.json").features;
        let mut dx_data: Vec<ClientFeature> = features
            .iter()
            .filter(|f| f.project == Some("dx".into()))
            .cloned()
            .collect();
        dx_data.remove(0);
        let mut eg_data: Vec<ClientFeature> = features
            .iter()
            .filter(|f| f.project == Some("eg".into()))
            .cloned()
            .collect();
        eg_data.remove(0);
        dx_data.extend(eg_data);
        let edge_token = EdgeToken {
            token: "".to_string(),
            token_type: Some(TokenType::Backend),
            environment: None,
            projects: vec![String::from("dx"), String::from("eg")],
            status: Validated,
        };
        let update = update_projects_from_feature_update(&edge_token, &features, &dx_data);
        assert_eq!(features.len() - update.len(), 2); // We've removed two elements
    }

    #[test]
    pub fn if_project_is_removed_but_token_has_access_to_project_update_should_remove_cached_project()
     {
        let features = features_from_disk("../../../examples/hostedexample.json").features;
        let edge_token = EdgeToken {
            token: "".to_string(),
            token_type: Some(TokenType::Backend),
            environment: None,
            projects: vec![String::from("dx"), String::from("eg")],
            status: Validated,
        };
        let eg_data: Vec<ClientFeature> = features
            .iter()
            .filter(|f| f.project == Some("eg".into()))
            .cloned()
            .collect();
        let update = update_projects_from_feature_update(&edge_token, &features, &eg_data);
        assert!(!update.iter().any(|p| p.project == Some(String::from("dx"))));
    }

    #[test]
    pub fn if_token_does_not_have_access_to_project_no_update_happens_to_project() {
        let features = features_from_disk("../../../examples/hostedexample.json").features;
        let edge_token = EdgeToken {
            token: "".to_string(),
            token_type: Some(TokenType::Backend),
            environment: None,
            projects: vec![String::from("dx"), String::from("eg")],
            status: Validated,
        };
        let eg_data: Vec<ClientFeature> = features
            .iter()
            .filter(|f| f.project == Some("eg".into()))
            .cloned()
            .collect();
        let update = update_projects_from_feature_update(&edge_token, &features, &eg_data);
        assert_eq!(
            update
                .iter()
                .filter(|p| p.project == Some(String::from("unleash-cloud")))
                .count(),
            1
        );
    }

    #[test]
    pub fn if_token_is_wildcard_our_entire_cache_is_replaced_by_update() {
        let features = vec![
            ClientFeature {
                name: "my.first.toggle.in.default".to_string(),
                feature_type: Some("release".into()),
                description: None,
                created_at: None,
                last_seen_at: None,
                enabled: true,
                stale: None,
                impression_data: None,
                project: Some("default".into()),
                strategies: None,
                variants: None,
                dependencies: None,
            },
            ClientFeature {
                name: "my.second.toggle.in.testproject".to_string(),
                feature_type: Some("release".into()),
                description: None,
                created_at: None,
                last_seen_at: None,
                enabled: false,
                stale: None,
                impression_data: None,
                project: Some("testproject".into()),
                strategies: None,
                variants: None,
                dependencies: None,
            },
        ];
        let edge_token = EdgeToken {
            token: "".to_string(),
            token_type: Some(TokenType::Backend),
            environment: None,
            projects: vec![String::from("*")],
            status: Validated,
        };
        let update: Vec<ClientFeature> = features
            .clone()
            .iter()
            .filter(|t| t.project == Some("default".into()))
            .cloned()
            .collect();
        let updated = update_projects_from_feature_update(&edge_token, &features, &update);
        assert_eq!(updated.len(), 1);
        assert!(updated.iter().all(|f| f.project == Some("default".into())))
    }

    #[test]
    pub fn token_with_access_to_different_project_than_exists_in_cache_should_never_delete_features_from_other_projects()
     {
        // Added after customer issue in May '24 when tokens unrelated to projects in cache with no actual features connected to them removed existing features in cache for unrelated projects
        let features = vec![
            ClientFeature {
                name: "my.first.toggle.in.default".to_string(),
                feature_type: Some("release".into()),
                description: None,
                created_at: None,
                last_seen_at: None,
                enabled: true,
                stale: None,
                impression_data: None,
                project: Some("testproject".into()),
                strategies: None,
                variants: None,
                dependencies: None,
            },
            ClientFeature {
                name: "my.second.toggle.in.testproject".to_string(),
                feature_type: Some("release".into()),
                description: None,
                created_at: None,
                last_seen_at: None,
                enabled: false,
                stale: None,
                impression_data: None,
                project: Some("testproject".into()),
                strategies: None,
                variants: None,
                dependencies: None,
            },
        ];
        let empty_features = vec![];
        let unrelated_token_to_existing_features = EdgeToken {
            token: "someotherproject:dev.myextralongsecretstringwithfeatures".to_string(),
            token_type: Some(TokenType::Backend),
            environment: Some("dev".into()),
            projects: vec![String::from("someother")],
            status: Validated,
        };
        let updated = update_projects_from_feature_update(
            &unrelated_token_to_existing_features,
            &features,
            &empty_features,
        );
        assert_eq!(updated.len(), 2);
    }

    #[test]
    pub fn token_with_access_to_both_a_different_project_than_exists_in_cache_and_the_cached_project_should_delete_features_from_both_projects()
     {
        // Added after customer issue in May '24 when tokens unrelated to projects in cache with no actual features connected to them removed existing features in cache for unrelated projects
        let features = vec![
            ClientFeature {
                name: "my.first.toggle.in.default".to_string(),
                feature_type: Some("release".into()),
                description: None,
                created_at: None,
                last_seen_at: None,
                enabled: true,
                stale: None,
                impression_data: None,
                project: Some("testproject".into()),
                strategies: None,
                variants: None,
                dependencies: None,
            },
            ClientFeature {
                name: "my.second.toggle.in.testproject".to_string(),
                feature_type: Some("release".into()),
                description: None,
                created_at: None,
                last_seen_at: None,
                enabled: false,
                stale: None,
                impression_data: None,
                project: Some("testproject".into()),
                strategies: None,
                variants: None,
                dependencies: None,
            },
        ];
        let empty_features = vec![];
        let token_with_access_to_both_empty_and_full_project = EdgeToken {
            token: "[]:dev.myextralongsecretstringwithfeatures".to_string(),
            token_type: Some(TokenType::Backend),
            environment: Some("dev".into()),
            projects: vec![String::from("testproject"), String::from("someother")],
            status: Validated,
        };
        let updated = update_projects_from_feature_update(
            &token_with_access_to_both_empty_and_full_project,
            &features,
            &empty_features,
        );
        assert_eq!(updated.len(), 0);
    }

    fn features_from_disk(path: &str) -> ClientFeatures {
        let path = PathBuf::from(path);
        let file = fs::File::open(path).unwrap();
        let reader = BufReader::new(file);
        serde_json::from_reader(reader).unwrap()
    }
}
