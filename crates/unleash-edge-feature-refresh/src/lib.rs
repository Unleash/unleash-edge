use std::collections::HashSet;
use std::{sync::Arc, time::Duration};

pub mod delta_refresh;

use chrono::Utc;
use dashmap::DashMap;
use etag::EntityTag;
use eventsource_client::Client;
use futures::TryStreamExt;
use json_structural_diff::JsonDiff;
use reqwest::StatusCode;
use tracing::{debug, info, warn};
use unleash_edge_delta::cache_manager::DeltaCacheManager;
use unleash_edge_feature_cache::FeatureCache;
use unleash_edge_feature_filters::delta_filters::{DeltaFilterSet, filter_delta_events};
use unleash_edge_feature_filters::{FeatureFilterSet, filter_client_features};
use unleash_edge_http_client::{ClientMetaInformation, UnleashClient};
use unleash_edge_persistence::EdgePersistence;
use unleash_edge_types::errors::{EdgeError, FeatureError};
use unleash_edge_types::headers::{
    UNLEASH_APPNAME_HEADER, UNLEASH_CLIENT_SPEC_HEADER, UNLEASH_CONNECTION_ID_HEADER,
    UNLEASH_INSTANCE_ID_HEADER,
};
use unleash_edge_types::tokens::{EdgeToken, cache_key, simplify};
use unleash_edge_types::{
    ClientFeaturesDeltaResponse, ClientFeaturesRequest, ClientFeaturesResponse, EdgeResult,
    TokenRefresh, TokenType, TokenValidationStatus, build,
};
use unleash_types::client_features::{ClientFeatures, ClientFeaturesDelta, DeltaEvent};
use unleash_types::client_metrics::{ClientApplication, MetricsMetadata, SdkType};
use unleash_yggdrasil::{EngineState, UpdateMessage};

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

    pub fn token_is_subsumed(&self, token: &EdgeToken) -> bool {
        self.tokens_to_refresh
            .iter()
            .filter(|r| r.token.environment == token.environment)
            .any(|t| t.token.subsumes(token))
    }

    pub fn frontend_token_is_covered_by_client_token(&self, frontend_token: &EdgeToken) -> bool {
        frontend_token_is_covered_by_tokens(frontend_token, self.tokens_to_refresh.clone())
    }

    /// This method no longer returns any data. Its responsibility lies in adding the token to our
    /// list of tokens to perform refreshes for, as well as calling out to hydrate tokens that we haven't seen before.
    /// Other tokens will be refreshed due to the scheduled task that refreshes tokens that haven been refreshed in ${refresh_interval} seconds
    pub async fn register_and_hydrate_token(&self, token: &EdgeToken) {
        self.register_token_for_refresh(token.clone(), None).await;
        self.hydrate_new_tokens().await;
    }

    pub async fn create_client_token_for_fe_token(&self, token: EdgeToken) -> EdgeResult<()> {
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

    pub fn features_for_filter(
        &self,
        token: EdgeToken,
        filters: &FeatureFilterSet,
    ) -> EdgeResult<ClientFeatures> {
        match self.get_features_by_filter(&token, &filters) {
            Some(features) if self.token_is_subsumed(&token) => Ok(features),
            Some(_features) if !self.token_is_subsumed(&token) => {
                debug!("Strict behavior: Token is not subsumed by any registered tokens. Returning error");
                Err(EdgeError::InvalidTokenWithStrictBehavior)
            }
            _ => {
                debug!(
                    "No features set available. Edge isn't ready"
                );
                Err(EdgeError::InvalidTokenWithStrictBehavior)
            }
        }
    }

    pub fn delta_events_for_filter(
        &self,
        token: EdgeToken,
        feature_filters: FeatureFilterSet,
        delta_filters: DeltaFilterSet,
        revision: u32,
    ) -> EdgeResult<ClientFeaturesDelta> {
        match self.get_delta_events_by_filter(&token, &feature_filters, &delta_filters, revision) {
            Some(features) if self.token_is_subsumed(&token) => Ok(features),
            _ => {
                debug!(
                    "Strict behavior: Token is not subsumed by any registered tokens. Returning error"
                );
                Err(EdgeError::InvalidTokenWithStrictBehavior)
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
        for hydration in self.get_tokens_never_refreshed() {
            if self.delta {
                self.refresh_single_delta(hydration).await;
            } else {
                info!("Refreshing {hydration:?}");
                self.refresh_single(hydration).await;
            }
        }
    }
    pub async fn refresh_features(&self) {
        let refreshes = self.get_tokens_due_for_refresh();
        info!("{:#?}", refreshes);
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
        info!("Refreshing {refresh:?}");
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
}
