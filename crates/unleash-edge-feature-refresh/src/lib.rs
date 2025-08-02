use std::collections::HashSet;
use std::{sync::Arc, time::Duration};

use etag::EntityTag;
use chrono::Utc;
use dashmap::DashMap;
use eventsource_client::Client;
use futures::TryStreamExt;
use json_structural_diff::JsonDiff;
use reqwest::StatusCode;
use tracing::{debug, info, warn};
use unleash_types::client_features::{ClientFeatures, ClientFeaturesDelta, DeltaEvent};
use unleash_types::client_metrics::{ClientApplication, MetricsMetadata, SdkType};
use unleash_yggdrasil::{EngineState, UpdateMessage};
use unleash_edge_types::TokenRefresh;
use unleash_edge_types::tokens::EdgeToken;

fn frontend_token_is_covered_by_tokens(
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
pub struct FeatureRefresher {
    pub unleash_client: Arc<UnleashClient>,
    pub tokens_to_refresh: Arc<DashMap<String, TokenRefresh>>,
    pub features_cache: Arc<FeatureCache>,
    pub delta_cache_manager: Arc<DeltaCacheManager>,
    pub engine_cache: Arc<DashMap<String, EngineState>>,
    pub refresh_interval: chrono::Duration,
    pub persistence: Option<Arc<dyn EdgePersistence>>,
    pub strict: bool,
    pub streaming: bool,
    pub client_meta_information: ClientMetaInformation,
    pub delta: bool,
    pub delta_diff: bool,
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
            strict: true,
            streaming: false,
            client_meta_information: Default::default(),
            delta: false,
            delta_diff: false,
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

#[derive(Eq, PartialEq)]
pub enum FeatureRefresherMode {
    Dynamic,
    Streaming,
    Strict,
}

pub struct FeatureRefreshConfig {
    features_refresh_interval: chrono::Duration,
    mode: FeatureRefresherMode,
    client_meta_information: ClientMetaInformation,
    delta: bool,
    delta_diff: bool,
}

impl FeatureRefreshConfig {
    pub fn new(
        features_refresh_interval: chrono::Duration,
        mode: FeatureRefresherMode,
        client_meta_information: ClientMetaInformation,
        delta: bool,
        delta_diff: bool,
    ) -> Self {
        Self {
            features_refresh_interval,
            mode,
            client_meta_information,
            delta,
            delta_diff,
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
            strict: config.mode != FeatureRefresherMode::Dynamic,
            streaming: config.mode == FeatureRefresherMode::Streaming,
            client_meta_information: config.client_meta_information,
            delta: config.delta,
            delta_diff: config.delta_diff,
        }
    }

    pub fn with_client(client: Arc<UnleashClient>) -> Self {
        Self {
            unleash_client: client,
            ..Default::default()
        }
    }

    pub(crate) fn get_tokens_due_for_refresh(&self) -> Vec<TokenRefresh> {
        self.tokens_to_refresh
            .iter()
            .map(|e| e.value().clone())
            .filter(|token| {
                token
                    .next_refresh
                    .map(|refresh| Utc::now() > refresh)
                    .unwrap_or(true)
            })
            .collect()
    }

    pub(crate) fn get_tokens_never_refreshed(&self) -> Vec<TokenRefresh> {
        self.tokens_to_refresh
            .iter()
            .map(|e| e.value().clone())
            .filter(|token| token.last_refreshed.is_none() && token.last_check.is_none())
            .collect()
    }

    pub(crate) fn token_is_subsumed(&self, token: &EdgeToken) -> bool {
        self.tokens_to_refresh
            .iter()
            .filter(|r| r.token.environment == token.environment)
            .any(|t| t.token.subsumes(token))
    }

    pub(crate) fn frontend_token_is_covered_by_client_token(
        &self,
        frontend_token: &EdgeToken,
    ) -> bool {
        frontend_token_is_covered_by_tokens(frontend_token, self.tokens_to_refresh.clone())
    }

    /// This method no longer returns any data. Its responsibility lies in adding the token to our
    /// list of tokens to perform refreshes for, as well as calling out to hydrate tokens that we haven't seen before.
    /// Other tokens will be refreshed due to the scheduled task that refreshes tokens that haven been refreshed in ${refresh_interval} seconds
    pub(crate) async fn register_and_hydrate_token(&self, token: &EdgeToken) {
        self.register_token_for_refresh(token.clone(), None).await;
        self.hydrate_new_tokens().await;
    }

    pub(crate) async fn create_client_token_for_fe_token(
        &self,
        token: EdgeToken,
    ) -> EdgeResult<()> {
        if token.status == TokenValidationStatus::Validated
            && token.token_type == Some(TokenType::Frontend)
        {
            if !self.frontend_token_is_covered_by_client_token(&token) {
                warn!("The frontend token access is not covered by our current client tokens");
                Err(EdgeError::EdgeTokenError)
            } else {
                debug!("It is already covered by an existing client token. Doing nothing");
                Ok(())
            }
        } else {
            debug!("Token is not validated or is not a frontend token. Doing nothing");
            Ok(())
        }
    }

    pub(crate) async fn features_for_filter(
        &self,
        token: EdgeToken,
        filters: &FeatureFilterSet,
    ) -> EdgeResult<ClientFeatures> {
        match self.get_features_by_filter(&token, filters) {
            Some(features) if self.token_is_subsumed(&token) => Ok(features),
            _ => {
                if self.strict {
                    debug!(
                        "Strict behavior: Token is not subsumed by any registered tokens. Returning error"
                    );
                    Err(EdgeError::InvalidTokenWithStrictBehavior)
                } else {
                    debug!(
                        "Dynamic behavior: Had never seen this environment. Configuring fetcher"
                    );
                    self.register_and_hydrate_token(&token).await;
                    self.get_features_by_filter(&token, filters).ok_or_else(|| {
                        EdgeError::ClientHydrationFailed(
                            "Failed to get features by filter after registering and hydrating token (This is very likely an error in Edge. Please report this!)"
                                .into(),
                        )
                    })
                }
            }
        }
    }

    pub(crate) async fn delta_events_for_filter(
        &self,
        token: EdgeToken,
        feature_filters: &FeatureFilterSet,
        delta_filters: &DeltaFilterSet,
        revision: u32,
    ) -> EdgeResult<ClientFeaturesDelta> {
        match self.get_delta_events_by_filter(&token, feature_filters, delta_filters, revision) {
            Some(features) if self.token_is_subsumed(&token) => Ok(features),
            _ => {
                if self.strict {
                    debug!(
                        "Strict behavior: Token is not subsumed by any registered tokens. Returning error"
                    );
                    Err(EdgeError::InvalidTokenWithStrictBehavior)
                } else {
                    debug!(
                        "Dynamic behavior: Had never seen this environment. Configuring fetcher"
                    );
                    self.register_and_hydrate_token(&token).await;
                    self.get_delta_events_by_filter(&token, feature_filters, delta_filters, revision).ok_or_else(|| {
                        EdgeError::ClientHydrationFailed(
                            "Failed to get delta events by filter after registering and hydrating token (This is very likely an error in Edge. Please report this!)"
                                .into(),
                        )
                    })
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

    fn get_delta_events_by_filter(
        &self,
        token: &EdgeToken,
        feature_filters: &FeatureFilterSet,
        delta_filters: &DeltaFilterSet,
        revision: u32,
    ) -> Option<ClientFeaturesDelta> {
        self.delta_cache_manager
            .get(&cache_key(token))
            .map(|delta_events| {
                filter_delta_events(&delta_events, feature_filters, delta_filters, revision)
            })
    }

    ///
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

    /// This is where we set up a listener per token.
    pub async fn start_streaming_features_background_task(
        &self,
        client_meta_information: ClientMetaInformation,
        custom_headers: Vec<(String, String)>,
    ) -> anyhow::Result<()> {
        use anyhow::Context;

        let refreshes = self.get_tokens_due_for_refresh();
        for refresh in refreshes {
            let token = refresh.token.clone();
            let streaming_url = self.unleash_client.urls.client_features_stream_url.as_str();

            let mut es_client_builder = eventsource_client::ClientBuilder::for_url(streaming_url)
                .context("Failed to create EventSource client for streaming")?
                .header("Authorization", &token.token)?
                .header(UNLEASH_APPNAME_HEADER, &client_meta_information.app_name)?
                .header(
                    UNLEASH_INSTANCE_ID_HEADER,
                    &client_meta_information.instance_id,
                )?
                .header(
                    UNLEASH_CONNECTION_ID_HEADER,
                    &client_meta_information.connection_id,
                )?
                .header(
                    UNLEASH_CLIENT_SPEC_HEADER,
                    unleash_yggdrasil::SUPPORTED_SPEC_VERSION,
                )?;

            for (key, value) in custom_headers.clone() {
                es_client_builder = es_client_builder.header(&key, &value)?;
            }

            let es_client = es_client_builder
                .reconnect(
                    eventsource_client::ReconnectOptions::reconnect(true)
                        .retry_initial(true)
                        .delay(Duration::from_secs(5))
                        .delay_max(Duration::from_secs(30))
                        .backoff_factor(2)
                        .build(),
                )
                .build();

            let refresher = self.clone();

            tokio::spawn(async move {
                let mut stream = es_client
                    .stream()
                    .map_ok(move |sse| {
                        let token = token.clone();
                        let refresher = refresher.clone();
                        async move {
                            match sse {
                                // The first time we're connecting to Unleash.
                                eventsource_client::SSE::Event(event)
                                if event.event_type == "unleash-connected" =>
                                    {
                                        debug!(
                                        "Connected to unleash! Populating my flag cache now.",
                                    );

                                        match serde_json::from_str(&event.data) {
                                            Ok(features) => { refresher.handle_client_features_updated(&token, features, None).await; }
                                            Err(e) => { tracing::error!("Could not parse features response to internal representation: {e:?}");
                                            }
                                        }
                                    }
                                // Unleash has updated features for us.
                                eventsource_client::SSE::Event(event)
                                if event.event_type == "unleash-updated" =>
                                    {
                                        debug!(
                                        "Got an unleash updated event. Updating cache.",
                                    );

                                        match serde_json::from_str(&event.data) {
                                            Ok(features) => { refresher.handle_client_features_updated(&token, features, None).await; }
                                            Err(e) => { warn!("Could not parse features response to internal representation: {e:?}");
                                            }
                                        }
                                    }
                                eventsource_client::SSE::Event(event) => {
                                    info!(
                                        "Got an SSE event that I wasn't expecting: {:#?}",
                                        event
                                    );
                                }
                                eventsource_client::SSE::Connected(_) => {
                                    debug!("SSE Connection established");
                                }
                                eventsource_client::SSE::Comment(_) => {
                                    // purposefully left blank.
                                },
                            }
                        }
                    })
                    .map_err(|e| warn!("Error in SSE stream: {:?}", e));

                loop {
                    match stream.try_next().await {
                        Ok(Some(handler)) => handler.await,
                        Ok(None) => {
                            info!("SSE stream ended? Handler was None, anyway. Reconnecting.");
                        }
                        Err(e) => {
                            info!("SSE stream error: {e:?}. Reconnecting");
                        }
                    }
                }
            });
        }
        Ok(())
    }

    async fn compare_delta_cache(&self, refresh: &TokenRefresh) {
        let delta_result = self
            .unleash_client
            .get_client_features_delta(ClientFeaturesRequest {
                api_key: refresh.token.token.clone(),
                etag: None,
                interval: None,
            })
            .await;

        let key = cache_key(&refresh.token);
        if let Some(client_features) = self.features_cache.get(&key).as_ref() {
            if let Ok(ClientFeaturesDeltaResponse::Updated(delta_features, _etag)) = delta_result {
                let c_features = &client_features.features;
                let d_features = delta_features.events.iter().find_map(|event| {
                    if let DeltaEvent::Hydration { features, .. } = event {
                        Some(features)
                    } else {
                        None
                    }
                });

                let delta_json = serde_json::to_value(d_features).unwrap();
                let client_json = serde_json::to_value(c_features).unwrap();

                let delta_json_len = delta_json.to_string().len();
                let client_json_len = client_json.to_string().len();

                if delta_json_len == client_json_len {
                    info!("The JSON structure lengths are identical.");
                } else {
                    info!("Structural differences found:");
                    info!("Length of delta_json: {}", delta_json_len);
                    info!("Length of old_json: {}", client_json_len);
                    let diff = JsonDiff::diff(&delta_json, &client_json, false);
                    debug!("{:?}", diff.diff.unwrap());
                }
            }
        }
    }

    pub async fn start_refresh_features_background_task(&self) {
        if self.streaming {
            loop {
                tokio::time::sleep(Duration::from_secs(3600)).await;
            }
        } else {
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(5)) => {
                        self.refresh_features().await;
                    }
                }
            }
        }
    }

    pub async fn hydrate_new_tokens(&self) {
        let hydrations = self.get_tokens_never_refreshed();
        for hydration in hydrations {
            if self.delta {
                self.refresh_single_delta(hydration).await;
            } else {
                self.refresh_single(hydration).await;
            }
        }
    }
    pub async fn refresh_features(&self) {
        let refreshes = self.get_tokens_due_for_refresh();
        for refresh in refreshes {
            if self.delta {
                self.refresh_single_delta(refresh).await;
            } else {
                self.refresh_single(refresh).await;
            }
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
        self.update_last_refresh(refresh_token, etag, features.features.len());
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
                    self.update_last_check(&refresh.token.clone());
                }
                ClientFeaturesResponse::Updated(features, etag) => {
                    self.handle_client_features_updated(&refresh.token, features, etag)
                        .await;
                    if self.delta_diff {
                        self.compare_delta_cache(&refresh).await;
                    }
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
                                    self.backoff(&refresh.token);
                                }
                                StatusCode::TOO_MANY_REQUESTS => {
                                    info!("Got told that upstream is receiving too many requests");
                                    self.backoff(&refresh.token);
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
                                self.backoff(&refresh.token);
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
    pub fn backoff(&self, token: &EdgeToken) {
        self.tokens_to_refresh
            .alter(&token.token, |_k, old_refresh| {
                old_refresh.backoff(&self.refresh_interval)
            });
    }
    pub fn update_last_check(&self, token: &EdgeToken) {
        self.tokens_to_refresh
            .alter(&token.token, |_k, old_refresh| {
                old_refresh.successful_check(&self.refresh_interval)
            });
    }

    pub fn update_last_refresh(
        &self,
        token: &EdgeToken,
        etag: Option<EntityTag>,
        feature_count: usize,
    ) {
        self.tokens_to_refresh
            .alter(&token.token, |_k, old_refresh| {
                old_refresh.successful_refresh(&self.refresh_interval, etag, feature_count)
            });
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;
    use std::sync::Arc;
    use axum::Router;
    use axum_test::TestServer;
    use chrono::{Duration, Utc};
    use dashmap::DashMap;
    use etag::EntityTag;
    use reqwest::Url;
    use unleash_types::client_features::ClientFeature;
    use unleash_yggdrasil::{EngineState, UpdateMessage};

    use crate::feature_cache::{FeatureCache, update_projects_from_feature_update};
    use crate::filters::{FeatureFilterSet, project_filter};
    use crate::http::unleash_client::{ClientMetaInformation, HttpClientArgs, new_reqwest_client};
    use crate::tests::features_from_disk;
    use crate::tokens::cache_key;
    use crate::types::TokenValidationStatus::Validated;
    use crate::types::{TokenType, TokenValidationStatus};
    use crate::{
        http::unleash_client::UnleashClient,
        types::{EdgeToken, TokenRefresh},
    };
    use crate::state::AppState;
    use super::{FeatureRefresher, frontend_token_is_covered_by_tokens};

    impl PartialEq for TokenRefresh {
        fn eq(&self, other: &Self) -> bool {
            self.token == other.token
                && self.etag == other.etag
                && self.last_refreshed == other.last_refreshed
                && self.last_check == other.last_check
        }
    }

    fn create_test_client() -> UnleashClient {
        let http_client = new_reqwest_client(HttpClientArgs {
            client_meta_information: ClientMetaInformation::test_config(),
            ..Default::default()
        })
            .expect("Failed to create client");

        UnleashClient::from_url(
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

        let duration = Duration::seconds(5);
        let feature_refresher = FeatureRefresher {
            unleash_client: Arc::new(unleash_client),
            features_cache,
            engine_cache,
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
            etag: Some(EntityTag::new(true, "abcde".into())),
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
            etag: Some(EntityTag::new(true, "abcde".into())),
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
        let tokens_to_refresh = feature_refresher.get_tokens_due_for_refresh();
        assert_eq!(tokens_to_refresh.len(), 2);
        assert!(tokens_to_refresh.contains(&etag_but_last_refreshed_ten_seconds_ago));
        assert!(tokens_to_refresh.contains(&no_etag_so_is_due_for_refresh));
    }

    async fn client_api_test_server(
        upstream_token_cache: Arc<DashMap<String, EdgeToken>>,
        upstream_features_cache: Arc<FeatureCache>,
        upstream_engine_cache: Arc<DashMap<String, EngineState>>,
    ) -> TestServer {
        let app_state = AppState::builder().with_token_cache(upstream_token_cache.clone())
            .with_features_cache(upstream_features_cache.clone())
            .with_engine_cache(upstream_engine_cache.clone())
            .build();
        let router = Router::new()
            .nest("/api", crate::client_api::router)
            .with_state(app_state);
        TestServer::builder().http_transport().build(router).expect("Failed to build client api test server")
    }
    #[tokio::test]
    pub async fn getting_403_when_refreshing_features_will_remove_token() {
        let upstream_features_cache: Arc<FeatureCache> = Arc::new(FeatureCache::default());
        let upstream_engine_cache: Arc<DashMap<String, EngineState>> = Arc::new(DashMap::default());
        let upstream_token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());
        let server = client_api_test_server(
            upstream_token_cache,
            upstream_features_cache,
            upstream_engine_cache,
        )
            .await;
        let unleash_client = UnleashClient::new(server.url("/").as_str(), None).unwrap();
        let features_cache: Arc<FeatureCache> = Arc::new(FeatureCache::default());
        let engine_cache: Arc<DashMap<String, EngineState>> = Arc::new(DashMap::default());
        let feature_refresher = FeatureRefresher {
            unleash_client: Arc::new(unleash_client),
            features_cache,
            engine_cache,
            refresh_interval: Duration::seconds(60),
            ..Default::default()
        };
        let mut token = EdgeToken::try_from("*:development.secret123".to_string()).unwrap();
        token.status = Validated;
        token.token_type = Some(TokenType::Client);
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
    pub async fn getting_404_removes_tokens_from_token_to_refresh_but_not_its_features() {
        let mut token = EdgeToken::try_from("*:development.secret123".to_string()).unwrap();
        token.status = Validated;
        token.token_type = Some(TokenType::Client);
        let token_cache = DashMap::default();
        token_cache.insert(token.token.clone(), token.clone());
        let upstream_features_cache: Arc<FeatureCache> = Arc::new(FeatureCache::default());
        let upstream_engine_cache: Arc<DashMap<String, EngineState>> = Arc::new(DashMap::default());
        let upstream_token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(token_cache);
        let example_features = features_from_disk("../examples/features.json");
        let cache_key = cache_key(&token);
        let mut engine_state = EngineState::default();
        let warnings =
            engine_state.take_state(UpdateMessage::FullResponse(example_features.clone()));
        upstream_features_cache.insert(cache_key.clone(), example_features.clone());
        upstream_engine_cache.insert(cache_key.clone(), engine_state);
        let mut server = client_api_test_server(
            upstream_token_cache,
            upstream_features_cache,
            upstream_engine_cache,
        )
            .await;
        let unleash_client = UnleashClient::new(server.url("/").as_str(), None).unwrap();
        let features_cache: Arc<FeatureCache> = Arc::new(FeatureCache::default());
        let engine_cache: Arc<DashMap<String, EngineState>> = Arc::new(DashMap::default());
        let feature_refresher = FeatureRefresher {
            unleash_client: Arc::new(unleash_client),
            features_cache,
            engine_cache,
            refresh_interval: Duration::milliseconds(1),
            ..Default::default()
        };
        feature_refresher
            .register_token_for_refresh(token, None)
            .await;
        assert!(!feature_refresher.tokens_to_refresh.is_empty());
        feature_refresher.refresh_features().await;
        assert!(!feature_refresher.tokens_to_refresh.is_empty());
        assert!(!feature_refresher.features_cache.is_empty());
        assert!(!feature_refresher.engine_cache.is_empty());
        server.stop().await;
        tokio::time::sleep(std::time::Duration::from_millis(5)).await; // To ensure our refresh is due
        feature_refresher.refresh_features().await;
        assert_eq!(
            feature_refresher
                .tokens_to_refresh
                .get("*:development.secret123")
                .unwrap()
                .failure_count,
            1
        );
        assert!(!feature_refresher.features_cache.is_empty());
        assert!(!feature_refresher.engine_cache.is_empty());
        assert!(warnings.is_none());
    }

    #[tokio::test]
    pub async fn when_we_have_a_cache_and_token_gets_removed_caches_are_emptied() {
        let upstream_features_cache: Arc<FeatureCache> = Arc::new(FeatureCache::default());
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
        let warnings =
            engine_state.take_state(UpdateMessage::FullResponse(example_features.clone()));
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
        let warnings =
            engine_state.take_state(UpdateMessage::FullResponse(example_features.clone()));
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

    #[tokio::test]
    pub async fn fetching_two_projects_from_same_environment_should_get_features_for_both_when_dynamic()
    {
        let upstream_features_cache: Arc<FeatureCache> = Arc::new(FeatureCache::default());
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
        let warnings =
            engine_state.take_state(UpdateMessage::FullResponse(example_features.clone()));
        upstream_engine_cache.insert(cache_key, engine_state);
        let server = client_api_test_server(
            upstream_token_cache,
            upstream_features_cache,
            upstream_engine_cache,
        )
            .await;
        let unleash_client = UnleashClient::new(server.url("/").as_str(), None).unwrap();
        let mut feature_refresher = FeatureRefresher::with_client(Arc::new(unleash_client));
        feature_refresher.strict = false;
        feature_refresher.refresh_interval = Duration::seconds(0);
        let dx_features = feature_refresher
            .features_for_filter(
                dx_token.clone(),
                &FeatureFilterSet::from(project_filter(&dx_token)),
            )
            .await
            .expect("No dx features");
        assert!(
            dx_features
                .features
                .iter()
                .all(|f| f.project == Some("dx".into()))
        );
        assert_eq!(dx_features.features.len(), 16);
        let eg_features = feature_refresher
            .features_for_filter(
                eg_token.clone(),
                &FeatureFilterSet::from(project_filter(&eg_token)),
            )
            .await
            .expect("Could not get eg features");
        assert_eq!(eg_features.features.len(), 7);
        assert!(
            eg_features
                .features
                .iter()
                .all(|f| f.project == Some("eg".into()))
        );
        assert!(warnings.is_none());
    }

    #[tokio::test]
    pub async fn should_get_data_for_multi_project_token_even_if_we_have_data_for_one_of_the_projects_when_dynamic()
    {
        let upstream_features_cache: Arc<FeatureCache> = Arc::new(FeatureCache::default());
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
        let warnings =
            engine_state.take_state(UpdateMessage::FullResponse(example_features.clone()));
        upstream_engine_cache.insert(cache_key, engine_state);
        let server = client_api_test_server(
            upstream_token_cache,
            upstream_features_cache,
            upstream_engine_cache,
        )
            .await;
        let unleash_client = UnleashClient::new(server.url("/").as_str(), None).unwrap();
        let mut feature_refresher = FeatureRefresher::with_client(Arc::new(unleash_client));
        feature_refresher.strict = false;
        feature_refresher.refresh_interval = Duration::seconds(0);
        let dx_features = feature_refresher
            .features_for_filter(
                dx_token.clone(),
                &FeatureFilterSet::from(project_filter(&dx_token)),
            )
            .await
            .expect("No dx features found");
        assert_eq!(dx_features.features.len(), 16);
        let unleash_cloud_features = feature_refresher
            .features_for_filter(
                multitoken.clone(),
                &FeatureFilterSet::from(project_filter(&multitoken)),
            )
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
            .features_for_filter(
                eg_token.clone(),
                &FeatureFilterSet::from(project_filter(&eg_token)),
            )
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
        eg_token.token_type = Some(TokenType::Client);
        eg_token.status = Validated;
        upstream_token_cache.insert(eg_token.token.clone(), eg_token.clone());
        let example_features = features_from_disk("../examples/hostedexample.json");
        let cache_key = cache_key(&eg_token);
        upstream_features_cache.insert(cache_key.clone(), example_features.clone());
        let mut engine_state = EngineState::default();
        let warnings =
            engine_state.take_state(UpdateMessage::FullResponse(example_features.clone()));
        upstream_engine_cache.insert(cache_key.clone(), engine_state);
        let server = client_api_test_server(
            upstream_token_cache,
            upstream_features_cache.clone(),
            upstream_engine_cache,
        )
            .await;
        let features_cache: Arc<FeatureCache> = Arc::new(FeatureCache::default());
        let unleash_client = UnleashClient::new(server.url("/").as_str(), None).unwrap();
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
        let empty_features = features_from_disk("../examples/empty-features.json");
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
        let features = features_from_disk("../examples/hostedexample.json").features;
        let mut dx_data: Vec<ClientFeature> = features_from_disk("../examples/hostedexample.json")
            .features
            .iter()
            .filter(|f| f.project == Some("dx".into()))
            .cloned()
            .collect();
        dx_data.remove(0);
        let mut token = EdgeToken::from_str("[]:development.somesecret").unwrap();
        token.status = TokenValidationStatus::Validated;
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
        let features = features_from_disk("../examples/hostedexample.json").features;
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
            token_type: Some(TokenType::Client),
            environment: None,
            projects: vec![String::from("dx"), String::from("eg")],
            status: TokenValidationStatus::Validated,
        };
        let update = update_projects_from_feature_update(&edge_token, &features, &dx_data);
        assert_eq!(features.len() - update.len(), 2); // We've removed two elements
    }

    #[test]
    pub fn if_project_is_removed_but_token_has_access_to_project_update_should_remove_cached_project()
    {
        let features = features_from_disk("../examples/hostedexample.json").features;
        let edge_token = EdgeToken {
            token: "".to_string(),
            token_type: Some(TokenType::Client),
            environment: None,
            projects: vec![String::from("dx"), String::from("eg")],
            status: TokenValidationStatus::Validated,
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
        let features = features_from_disk("../examples/hostedexample.json").features;
        let edge_token = EdgeToken {
            token: "".to_string(),
            token_type: Some(TokenType::Client),
            environment: None,
            projects: vec![String::from("dx"), String::from("eg")],
            status: TokenValidationStatus::Validated,
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
            token_type: Some(TokenType::Client),
            environment: None,
            projects: vec![String::from("*")],
            status: TokenValidationStatus::Validated,
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
            token_type: Some(TokenType::Client),
            environment: Some("dev".into()),
            projects: vec![String::from("someother")],
            status: TokenValidationStatus::Validated,
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
            token_type: Some(TokenType::Client),
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
}
