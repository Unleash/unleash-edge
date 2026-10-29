use crate::refresh_metrics::{
    DELTA_SOURCE, initialize_feature_refresh_metrics, observe_feature_refresh_error,
    observe_feature_state_warnings, observe_last_applied_revision_id,
};
use crate::{TokenRefreshSet, TokenRefreshStatus, client_application_from_token_and_name};
use anyhow::Context;
use chrono::Utc;
use dashmap::DashMap;
use etag::EntityTag;
use eventsource_client::{Client, Error};
use futures::StreamExt;
use prometheus::{IntGaugeVec, register_int_gauge_vec};
use reqwest::StatusCode;
use std::collections::HashSet;
use std::pin::Pin;
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};
use tokio::sync::watch::Receiver;
use tokio::time::{sleep, timeout};
use tracing::{debug, error, info, warn};
use unleash_edge_delta::cache::DeltaCache;
use unleash_edge_delta::cache_manager::DeltaCacheManager;
use unleash_edge_feature_cache::FeatureCache;
use unleash_edge_feature_filters::FeatureFilterSet;
use unleash_edge_feature_filters::delta_filters::{DeltaFilterSet, filter_delta_events};
use unleash_edge_http_client::{ClientMetaInformation, UnleashClient};
use unleash_edge_types::errors::{EdgeError, FeatureError};
use unleash_edge_types::headers::{
    UNLEASH_APPNAME_HEADER, UNLEASH_CLIENT_SPEC_HEADER, UNLEASH_CONNECTION_ID_HEADER,
    UNLEASH_INSTANCE_ID_HEADER,
};
use unleash_edge_types::metrics::instance_data::EdgeInstanceData;
use unleash_edge_types::tokens::{EdgeToken, cache_key, simplify};
use unleash_edge_types::{
    ClientFeaturesDeltaResponse, ClientFeaturesRequest, EdgeResult, RefreshState, TokenRefresh,
};
use unleash_types::client_features::ClientFeaturesDelta;
use unleash_yggdrasil::EngineState;

pub type Environment = String;

pub const DELTA_CACHE_LIMIT: usize = 100;
const SDK_TYPE_HEADER: &str = "Unleash-Sdk-Type";
const SSE_IDLE_TIMEOUT: Duration = Duration::from_secs(90);
const SSE_IDLE_RECONNECT_DELAY: Duration = Duration::from_secs(5);

#[derive(Clone, Copy)]
struct SseIdleConfig {
    timeout: Duration,
    reconnect_delay: Duration,
    state_lag_threshold: Option<Duration>,
}

impl Default for SseIdleConfig {
    fn default() -> Self {
        Self {
            timeout: SSE_IDLE_TIMEOUT,
            reconnect_delay: SSE_IDLE_RECONNECT_DELAY,
            state_lag_threshold: (std::env::var("EDGE_STATE_CHECKPOINTS").as_deref() == Ok("true"))
                .then(|| {
                    Duration::from_secs(
                        std::env::var("EDGE_STATE_LAG_THRESHOLD_SECONDS")
                            .ok()
                            .and_then(|v| v.parse::<u64>().ok())
                            .filter(|v| *v > 0)
                            .unwrap_or(30),
                    )
                }),
        }
    }
}

static DELTA_REVISION_ID: LazyLock<IntGaugeVec> = LazyLock::new(|| {
    register_int_gauge_vec!(
        "delta_revision_id",
        "Revision ID for delta refreshes",
        &["environment", "projects"]
    )
    .unwrap()
});

static DELTA_LAST_UPDATE: LazyLock<IntGaugeVec> = LazyLock::new(|| {
    register_int_gauge_vec!(
        "delta_last_update",
        "Timestamp for last update over delta",
        &["environment", "projects"]
    )
    .unwrap()
});

type SseStream = Pin<
    Box<
        dyn futures::Stream<Item = Result<eventsource_client::SSE, eventsource_client::Error>>
            + Send,
    >,
>;

fn reconnect_opts() -> eventsource_client::ReconnectOptions {
    eventsource_client::ReconnectOptions::reconnect(true)
        .retry_initial(true)
        .delay(Duration::from_secs(5))
        .delay_max(Duration::from_secs(30))
        .backoff_factor(2)
        .build()
}

fn build_sse_stream(
    streaming_url: &str,
    token: &EdgeToken,
    client_meta_information: &ClientMetaInformation,
    custom_headers: &[(String, String)],
    last_event_id: Option<&str>,
) -> anyhow::Result<SseStream> {
    debug!(
        "Building upstream SSE stream (url: {}, last_event_id: {})",
        streaming_url,
        last_event_id.unwrap_or("<none>")
    );
    let mut es_client_builder = eventsource_client::ClientBuilder::for_url(streaming_url)
        .context("Failed to create EventSource client for streaming")?
        .header("Authorization", &token.token)?
        .header(UNLEASH_APPNAME_HEADER, &client_meta_information.app_name)?
        .header(
            UNLEASH_INSTANCE_ID_HEADER,
            &client_meta_information.instance_id.to_string(),
        )?
        .header(SDK_TYPE_HEADER, "edge")?
        .header(
            UNLEASH_CONNECTION_ID_HEADER,
            &client_meta_information.connection_id.to_string(),
        )?
        .header(
            UNLEASH_CLIENT_SPEC_HEADER,
            unleash_yggdrasil::SUPPORTED_SPEC_VERSION,
        )?;

    if let Some(id) = last_event_id
        && !id.is_empty()
    {
        es_client_builder = es_client_builder.last_event_id(id.to_string());
    }

    for (key, value) in custom_headers {
        es_client_builder = es_client_builder.header(key, value)?;
    }

    let _ = rustls::crypto::ring::default_provider().install_default();
    let transport = launchdarkly_sdk_transport::HyperTransport::new_https()
        .context("Failed to create EventSource HTTPS transport")?;
    let client = es_client_builder
        .reconnect(reconnect_opts())
        .build_with_transport(transport);
    Ok(client.stream())
}

async fn handle_sse(
    sse: eventsource_client::SSE,
    delta_refresher: &Arc<DeltaRefresher>,
    token: &EdgeToken,
) -> Option<String> {
    match sse {
        eventsource_client::SSE::Event(event)
            if event.event_type == "unleash-connected" || event.event_type == "unleash-updated" =>
        {
            debug!(
                "Upstream SSE event received (type: {}, id: {})",
                event.event_type,
                event.id.as_deref().unwrap_or("<none>")
            );
            if event.event_type == "unleash-connected" {
                debug!("Connected to unleash! Populating flag cache now.");
            } else {
                debug!("Got an unleash updated event. Updating cache.");
            }

            match serde_json::from_str(&event.data) {
                Ok(delta) => {
                    delta_refresher
                        .handle_client_features_delta_updated(token, delta, None)
                        .await;
                    return event.id;
                }
                Err(e) => {
                    observe_feature_refresh_error(
                        token.environment.as_deref().unwrap_or("*"),
                        "parse",
                    );
                    warn!("Could not parse features response to internal representation: {e:?}");
                }
            }
        }
        eventsource_client::SSE::Event(event) => {
            info!("Got an SSE event that I wasn't expecting: {:#?}", event);
        }
        eventsource_client::SSE::Connected(_) => {
            debug!("SSE Connection established");
        }
        eventsource_client::SSE::Comment(_) => {
            // purposefully left blank.
        }
    }
    None
}

async fn run_stream_task(
    delta_refresher: Arc<DeltaRefresher>,
    token: EdgeToken,
    streaming_url: String,
    client_meta_information: ClientMetaInformation,
    custom_headers: Vec<(String, String)>,
    refresh_state_rx: Receiver<RefreshState>,
) {
    run_stream_task_with_idle_timeout(
        delta_refresher,
        token,
        streaming_url,
        client_meta_information,
        custom_headers,
        refresh_state_rx,
        SseIdleConfig::default(),
    )
    .await;
}

async fn run_stream_task_with_idle_timeout(
    delta_refresher: Arc<DeltaRefresher>,
    token: EdgeToken,
    streaming_url: String,
    client_meta_information: ClientMetaInformation,
    custom_headers: Vec<(String, String)>,
    mut refresh_state_rx: Receiver<RefreshState>,
    idle_config: SseIdleConfig,
) {
    initialize_feature_refresh_metrics(token.environment.as_deref().unwrap_or("*"));
    let mut stream: Option<SseStream> = None;
    let mut last_event_id: Option<String> = None;
    let mut verification = idle_config
        .state_lag_threshold
        .map(crate::stream_state::Verification::new);
    let mut verification_tick = tokio::time::interval(Duration::from_secs(1));
    verification_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut last_activity = Instant::now();

    loop {
        let state = *refresh_state_rx.borrow_and_update();
        if matches!(state, RefreshState::Paused) {
            if refresh_state_rx.changed().await.is_err() {
                info!("Refresh state channel closed; stopping SSE stream task");
                return;
            }
            continue;
        }

        if stream.is_none() {
            let s: SseStream = match build_sse_stream(
                &streaming_url,
                &token,
                &client_meta_information,
                &custom_headers,
                last_event_id.as_deref(),
            ) {
                Ok(s) => s,
                Err(e) => {
                    observe_feature_refresh_error(
                        token.environment.as_deref().unwrap_or("*"),
                        "stream",
                    );
                    warn!(
                        "SSE misconfiguration detected; cannot build stream: {e:?}. Exiting stream task."
                    );
                    return;
                }
            };

            stream = Some(s);
            last_activity = Instant::now();
            info!(
                "SSE connected (app: {}, instance: {})",
                client_meta_information.app_name, client_meta_information.instance_id
            );
        }

        if let Some(s) = stream.as_mut() {
            tokio::select! {
                result = refresh_state_rx.changed() => {
                    if result.is_ok() {
                        continue;
                    } else {
                        info!("Refresh state channel closed; stopping SSE stream task");
                        return;
                    }
                }
                _ = verification_tick.tick(), if verification.is_some() => {
                    let tracker = verification.as_mut().unwrap();
                    if let Some(reason) = tracker.recovery(Instant::now()) {
                        warn!("Upstream state verification requested fresh hydration ({reason})");
                        // Construct a new EventSource client with no replay cursor.
                        // Reusing the old client would retain its internal Last-Event-ID.
                        stream = None;
                        last_event_id = None;
                        sleep(idle_config.reconnect_delay).await;
                    }
                }
                next = timeout(idle_config.timeout.saturating_sub(last_activity.elapsed()), s.next()) => {
                    match next {
                        Err(_) => {
                            info!(
                                "SSE stream idle for {:?}; reconnecting after {:?}",
                                idle_config.timeout,
                                idle_config.reconnect_delay
                            );
                            stream = None;
                            if let Some(tracker) = &mut verification { tracker.disconnected(); }
                            sleep(idle_config.reconnect_delay).await;
                            continue;
                        }
                        Ok(Some(Ok(sse))) => {
                            last_activity = Instant::now();
                            if let eventsource_client::SSE::Event(event) = &sse
                                && event.event_type == "unleash-state" {
                                if let Some(tracker) = &mut verification {
                                    tracker.checkpoint(&event.data, Instant::now(), ||
                                        delta_refresher.state_fingerprint(&token));
                                }
                                continue;
                            }
                            if matches!(&sse, eventsource_client::SSE::Connected(_))
                                && let Some(tracker) = &mut verification { tracker.disconnected(); }
                            if let Some(id) =
                                handle_sse(sse, &delta_refresher, &token).await
                            {
                                if let Some(tracker) = &mut verification {
                                    tracker.applied(&id, Instant::now(), ||
                                        delta_refresher.state_fingerprint(&token));
                                }
                                last_event_id = Some(id);
                            }
                        }
                        Ok(Some(Err(e))) => {
                            observe_feature_refresh_error(
                                token.environment.as_deref().unwrap_or("*"),
                                "stream",
                            );
                            match e {
                                Error::UnexpectedResponse(response, _) => {
                                    if response.status() == StatusCode::UNAUTHORIZED || response.status() == StatusCode::FORBIDDEN {
                                        error!("Streaming was rejected with status code {}; stopping stream task", response.status());
                                        return;
                                    }
                                },
                                _ => {
                                    info!("SSE stream error: {e:?}; reconnecting immediately");
                                    stream = None;
                                    if let Some(tracker) = &mut verification { tracker.disconnected(); }
                                    continue;
                                }
                            }
                        }
                        Ok(None) => {
                            info!("SSE stream ended; reconnecting immediately");
                            stream = None;
                            if let Some(tracker) = &mut verification { tracker.disconnected(); }
                            continue;
                        }
                    }
                }
            }
        }
    }
}

pub async fn start_streaming_delta_background_task(
    delta_refresher: Arc<DeltaRefresher>,
    client_meta_information: ClientMetaInformation,
    custom_headers: Vec<(String, String)>,
    refresh_state_rx: Receiver<RefreshState>,
) -> anyhow::Result<()> {
    let refreshes = delta_refresher
        .tokens_to_refresh
        .clone()
        .iter()
        .map(|e| e.value().clone())
        .collect::<Vec<_>>();

    debug!("Spawning refreshers for {} tokens", refreshes.len());

    for refresh in refreshes {
        let token = refresh.token;
        let streaming_url = delta_refresher
            .unleash_client
            .urls
            .client_features_stream_url
            .to_string();

        let refresher = delta_refresher.clone();
        let client_meta_information = client_meta_information.clone();
        let custom_headers = custom_headers.clone();
        let refresh_state_rx = refresh_state_rx.clone();

        tokio::spawn(async move {
            run_stream_task(
                refresher,
                token,
                streaming_url,
                client_meta_information,
                custom_headers,
                refresh_state_rx,
            )
            .await;
        });
    }

    Ok(())
}

pub struct DeltaRefresher {
    pub unleash_client: Arc<UnleashClient>,
    pub tokens_to_refresh: TokenRefreshSet,
    pub features_cache: Arc<FeatureCache>,
    pub delta_cache_manager: Arc<DeltaCacheManager>,
    pub engine_cache: Arc<DashMap<String, EngineState>>,
    pub refresh_interval: chrono::Duration,
    pub streaming: bool,
    pub client_meta_information: ClientMetaInformation,
    pub edge_instance_data: Arc<EdgeInstanceData>,
}

impl DeltaRefresher {
    fn state_fingerprint(&self, token: &EdgeToken) -> Option<String> {
        // The cache is shared by environment, including segments. Another
        // upstream scope may already have applied newer content. Without a
        // per-scope snapshot we cannot call that a confirmed content mismatch.
        if self.tokens_to_refresh.iter().any(|entry| {
            entry.token.token != token.token && entry.token.environment == token.environment
        }) {
            return None;
        }
        crate::stream_state::fingerprint(&self.features_cache, token)
    }

    pub async fn hydrate_new_tokens(&self) {
        let tokens_never_refreshed = self.tokens_to_refresh.get_tokens_never_refreshed();

        for hydration in tokens_never_refreshed {
            self.refresh_single_delta(hydration).await;
        }
    }

    pub async fn refresh_features(&self) {
        let tokens_due_for_refresh = self.tokens_to_refresh.get_tokens_due_for_refresh();
        for refresh in tokens_due_for_refresh {
            self.refresh_single_delta(refresh).await;
        }
    }

    async fn handle_client_features_delta_updated(
        &self,
        refresh_token: &EdgeToken,
        mut delta: ClientFeaturesDelta,
        etag: Option<EntityTag>,
    ) {
        let updated_len = delta.events.len();

        debug!(
            "Got updated client features delta. Updating features with etag {etag:?}, events count {updated_len}"
        );

        let key: String = cache_key(refresh_token);
        if delta.events.iter().any(|e| {
            matches!(
                e,
                unleash_types::client_features::DeltaEvent::Hydration { .. }
            )
        }) {
            // Fresh hydration is authoritative for this token's projects only.
            // Preserve other upstream tokens sharing the environment cache.
            let snapshot =
                unleash_types::client_features::ClientFeatures::create_from_delta(&delta);
            if refresh_token.projects.iter().any(|p| p == "*") {
                self.features_cache.insert(key.clone(), snapshot);
            } else {
                self.features_cache
                    .modify(key.clone(), refresh_token, snapshot);
            }
            let combined = self.features_cache.get(&key).unwrap();
            delta = ClientFeaturesDelta {
                events: vec![unleash_types::client_features::DeltaEvent::Hydration {
                    event_id: delta
                        .events
                        .iter()
                        .map(|e| e.get_event_id())
                        .max()
                        .unwrap_or(0),
                    features: combined.features.clone(),
                    segments: combined.segments.clone().unwrap_or_default(),
                }],
            };
        } else {
            self.features_cache.apply_delta(key.clone(), &delta);
        }

        if !self.delta_cache_manager.contains(&key)
            && let Some(delta_cache) = DeltaCache::from_events(&delta.events, DELTA_CACHE_LIMIT)
        {
            self.delta_cache_manager.insert_cache(&key, delta_cache);
        }

        if self.delta_cache_manager.contains(&key) {
            self.delta_cache_manager.update_cache(&key, &delta.events);
        } else {
            warn!(
                "Warning: No hydrationEvent found in delta.events, but cache empty for environment"
            );
        }
        let max_event_id = delta
            .events
            .iter()
            .map(|e| e.get_event_id())
            .max()
            .map(|e| e as usize);
        if let (Some(max), Some(env)) = (max_event_id, refresh_token.environment.as_ref()) {
            DELTA_REVISION_ID
                .with_label_values(&[env, &refresh_token.projects.join(",")])
                .set(max as i64);
            self.edge_instance_data.observe_api_key_refresh(
                env.clone(),
                refresh_token.projects.clone(),
                max,
                Utc::now(),
            )
        }

        self.tokens_to_refresh.update_last_refresh(
            refresh_token,
            etag,
            self.features_cache.get(&key).unwrap().features.len(),
            max_event_id,
            &self.refresh_interval,
        );
        DELTA_LAST_UPDATE
            .with_label_values(&[
                refresh_token.environment.as_deref().unwrap_or("*"),
                &refresh_token.projects.join(","),
            ])
            .set(Utc::now().timestamp());
        self.engine_cache
            .entry(key.clone())
            .and_modify(|engine| {
                let warnings = engine.apply_delta(&delta);
                if let Some(warnings) = warnings {
                    observe_feature_state_warnings(
                        refresh_token.environment.as_deref().unwrap_or("*"),
                        DELTA_SOURCE,
                        warnings.len(),
                    );
                    warn!(
                        "The following toggle failed to compile and will be defaulted to off: {warnings:?}"
                    );
                };
            })
            .or_insert_with(|| {
                let new_state = EngineState::default();

                let warnings = new_state.apply_delta(&delta);
                if let Some(warnings) = warnings {
                    observe_feature_state_warnings(
                        refresh_token.environment.as_deref().unwrap_or("*"),
                        DELTA_SOURCE,
                        warnings.len(),
                    );
                    warn!("The following toggle failed to compile and will be defaulted to off: {warnings:?}");
                };
                new_state
            });
        if let (Some(max), Some(env)) = (max_event_id, refresh_token.environment.as_deref()) {
            observe_last_applied_revision_id(env, max);
        }
    }

    /// Registers a token for refresh, the token will be discarded if it can be subsumed by another previously registered token
    pub async fn register_token_for_refresh(&self, token: EdgeToken, etag: Option<EntityTag>) {
        initialize_feature_refresh_metrics(token.environment.as_deref().unwrap_or("*"));
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

    /// This method no longer returns any data. Its responsibility lies in adding the token to our
    /// list of tokens to perform refreshes for, as well as calling out to hydrate tokens that we haven't seen before.
    /// Other tokens will be refreshed due to the scheduled task that refreshes tokens that haven been refreshed in ${refresh_interval} seconds
    pub async fn register_and_hydrate_token(&self, token: &EdgeToken) {
        self.register_token_for_refresh(token.clone(), None).await;
        self.hydrate_new_tokens().await;
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

    pub fn delta_events_for_filter(
        &self,
        token: EdgeToken,
        feature_filters: FeatureFilterSet,
        delta_filters: DeltaFilterSet,
        revision: u32,
    ) -> EdgeResult<ClientFeaturesDelta> {
        match self.get_delta_events_by_filter(&token, &feature_filters, &delta_filters, revision) {
            Some(features) if self.tokens_to_refresh.token_is_subsumed(&token) => Ok(features),
            _ => {
                debug!("Token is not subsumed by any registered tokens. Returning error");
                Err(EdgeError::InvalidToken)
            }
        }
    }

    pub async fn refresh_single_delta(&self, refresh: TokenRefresh) {
        initialize_feature_refresh_metrics(refresh.token.environment.as_deref().unwrap_or("*"));
        let delta_result = self
            .unleash_client
            .get_client_features_delta(ClientFeaturesRequest {
                api_key: refresh.token.token.clone(),
                etag: refresh.etag,
                interval: Some(self.refresh_interval.num_milliseconds()),
            })
            .await;
        match delta_result {
            Ok(delta_response) => match delta_response {
                ClientFeaturesDeltaResponse::NoUpdate(tag) => {
                    debug!("No update needed. Will update last check time with {tag}");
                    self.tokens_to_refresh
                        .update_last_check(&refresh.token.clone(), &self.refresh_interval);
                }
                ClientFeaturesDeltaResponse::Updated(features, etag) => {
                    self.handle_client_features_delta_updated(&refresh.token, features, etag)
                        .await
                }
            },
            Err(e) => {
                observe_feature_refresh_error(
                    refresh.token.environment.as_deref().unwrap_or("*"),
                    "fetch",
                );
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
    use crate::delta_refresh::DeltaRefresher;
    use crate::refresh_metrics::{DELTA_SOURCE, feature_state_warnings_total};
    use axum::Router;
    use axum::body::Body;
    use axum::extract::Request;
    use axum::extract::State;
    use axum::response::{IntoResponse, Response};
    use axum::routing::get;
    use axum_test::TestServer;
    use chrono::Duration;
    use dashmap::DashMap;
    use etag::EntityTag;
    use futures::StreamExt as FuturesStreamExt;
    use http::StatusCode;
    use reqwest::Url;
    use std::convert::Infallible;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration as StdDuration;
    use tokio::sync::{Mutex, oneshot, watch};
    use ulid::Ulid;
    use unleash_edge_delta::cache_manager::DeltaCacheManager;
    use unleash_edge_feature_cache::FeatureCache;
    use unleash_edge_http_client::{
        ClientMetaInformation, HttpClientArgs, UnleashClient, new_reqwest_client,
    };
    use unleash_edge_types::entity_tag_to_header_value;
    use unleash_edge_types::metrics::instance_data::{EdgeInstanceData, Hosting};
    use unleash_edge_types::tokens::EdgeToken;
    use unleash_edge_types::{RefreshState, TokenRefresh};
    use unleash_types::client_features::{
        ClientFeature, ClientFeatures, ClientFeaturesDelta, Constraint, DeltaEvent, Operator,
        Segment, Strategy,
    };
    use unleash_yggdrasil::EngineState;

    trait TestConfig {
        fn test_config() -> Self;
    }

    fn edge_instance_data_for_delta_refresher_test() -> EdgeInstanceData {
        EdgeInstanceData {
            identifier: Ulid::generate().to_string(),
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
            enricher_metrics: None,
        }
    }

    impl TestConfig for ClientMetaInformation {
        fn test_config() -> Self {
            ClientMetaInformation {
                app_name: "test_app".into(),
                instance_id: Ulid::generate(),
                connection_id: Ulid::generate(),
            }
        }
    }

    pub fn build_unleash_client(server_url: Url) -> Arc<UnleashClient> {
        Arc::new(UnleashClient::from_url_with_backing_client(
            server_url,
            "Authorization".to_string(),
            new_reqwest_client(HttpClientArgs {
                skip_ssl_verification: false,
                client_identity: None,
                upstream_certificate_file: None,
                connect_timeout: Duration::seconds(10),
                socket_timeout: Duration::seconds(10),
                keep_alive_timeout: Duration::seconds(10),
                client_meta_information: ClientMetaInformation::test_config(),
            })
            .unwrap(),
            ClientMetaInformation::test_config(),
        ))
    }

    #[tokio::test]
    #[tracing_test::traced_test]
    async fn test_delta() {
        let srv = test_features_server().await;
        let unleash_client = build_unleash_client(srv.server_url("/").unwrap());
        let features_cache: Arc<FeatureCache> = Arc::new(FeatureCache::default());
        let delta_cache_manager: Arc<DeltaCacheManager> = Arc::new(DeltaCacheManager::new());
        let engine_cache: Arc<DashMap<String, EngineState>> = Arc::new(DashMap::default());
        let tokens_to_refresh = Arc::new(DashMap::default());

        let delta_refresher = Arc::new(DeltaRefresher {
            unleash_client: unleash_client.clone(),
            tokens_to_refresh,
            delta_cache_manager,
            features_cache: features_cache.clone(),
            engine_cache: engine_cache.clone(),
            refresh_interval: Duration::seconds(6000),
            streaming: false,
            client_meta_information: ClientMetaInformation::test_config(),
            edge_instance_data: Arc::new(edge_instance_data_for_delta_refresher_test()),
        });
        let mut delta_features = ClientFeatures::create_from_delta(&revision(1));
        let token =
            EdgeToken::try_from("*:development.abcdefghijklmnopqrstuvwxyz".to_string()).unwrap();
        delta_refresher
            .register_token_for_refresh(token.clone(), None)
            .await;
        delta_refresher.refresh_features().await;
        let refreshed_features = features_cache
            .get(&cache_key(&token))
            .unwrap()
            .value()
            .clone();
        assert_eq!(refreshed_features, delta_features);

        let token_refresh = delta_refresher
            .tokens_to_refresh
            .get(&token.token)
            .unwrap()
            .clone();
        delta_refresher.refresh_single_delta(token_refresh).await;
        let refreshed_features = features_cache
            .get(&cache_key(&token))
            .unwrap()
            .value()
            .clone();
        delta_features.apply_delta(&revision(2));
        assert_eq!(refreshed_features, delta_features);
    }

    #[tokio::test]
    async fn upstream_hydration_replaces_existing_delta_cache_snapshot() {
        let (delta_refresher, tokens_to_refresh) =
            build_delta_refresher_for_stream_test(Url::parse("http://localhost").unwrap());
        let token =
            EdgeToken::try_from("*:development.abcdefghijklmnopqrstuvwxyz".to_string()).unwrap();
        tokens_to_refresh.insert(token.token.clone(), TokenRefresh::new(token.clone(), None));
        let key = cache_key(&token);

        delta_refresher
            .handle_client_features_delta_updated(&token, hydration_delta(1, &["initial"]), None)
            .await;
        delta_refresher
            .handle_client_features_delta_updated(
                &token,
                ClientFeaturesDelta {
                    events: vec![feature_updated_event(2, "stale-only")],
                },
                None,
            )
            .await;
        delta_refresher
            .handle_client_features_delta_updated(&token, hydration_delta(9, &["rolled-up"]), None)
            .await;
        delta_refresher
            .handle_client_features_delta_updated(
                &token,
                ClientFeaturesDelta {
                    events: vec![feature_updated_event(10, "post-hydration")],
                },
                None,
            )
            .await;

        let cache = delta_refresher
            .delta_cache_manager
            .get(&key)
            .expect("delta cache should exist");
        let hydration_event = cache.get_hydration_event();
        let feature_names = hydration_event
            .features
            .iter()
            .map(|feature| feature.name.as_str())
            .collect::<Vec<_>>();

        assert_eq!(hydration_event.event_id, 10);
        assert_eq!(feature_names, vec!["rolled-up", "post-hydration"]);
    }

    #[tokio::test]
    async fn delta_warnings_increment_feature_state_warnings_metric() {
        let (delta_refresher, tokens_to_refresh) =
            build_delta_refresher_for_stream_test(Url::parse("http://localhost").unwrap());
        let token =
            EdgeToken::try_from("*:development.abcdefghijklmnopqrstuvwxyz".to_string()).unwrap();
        tokens_to_refresh.insert(token.token.clone(), TokenRefresh::new(token.clone(), None));
        let delta = delta_with_invalid_single_value_constraint(1);
        let expected_increment = EngineState::default()
            .apply_delta(&delta)
            .map_or(0, |warnings| {
                u64::try_from(warnings.len()).expect("warning count should fit in u64")
            });

        let initial = feature_state_warnings_total("development", DELTA_SOURCE);
        delta_refresher
            .handle_client_features_delta_updated(&token, delta, None)
            .await;

        assert_eq!(
            feature_state_warnings_total("development", DELTA_SOURCE),
            initial + expected_increment
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn applied_revision_waits_for_engine_update() {
        use crate::refresh_metrics::{last_applied_revision_id, observe_last_applied_revision_id};

        let (refresher, tokens) =
            build_delta_refresher_for_stream_test(Url::parse("http://localhost").unwrap());
        let token = EdgeToken::try_from("*:revision-order-test.secret".to_string()).unwrap();
        let environment = token.environment.as_deref().unwrap();
        tokens.insert(token.token.clone(), TokenRefresh::new(token.clone(), None));
        refresher
            .engine_cache
            .insert(cache_key(&token), EngineState::default());
        observe_last_applied_revision_id(environment, 1);

        // Hold the engine shard while the worker processes the received revision.
        // It must continue reporting the old revision until the engine can change.
        let engine = refresher.engine_cache.get(&cache_key(&token)).unwrap();
        let worker_refresher = refresher.clone();
        let worker_token = token.clone();
        let runtime = tokio::runtime::Handle::current();
        let worker = tokio::task::spawn_blocking(move || {
            runtime.block_on(worker_refresher.handle_client_features_delta_updated(
                &worker_token,
                hydration_delta(2, &["updated"]),
                None,
            ));
        });
        let received = tokio::time::timeout(StdDuration::from_secs(5), async {
            while tokens.get(&token.token).unwrap().last_refreshed.is_none() {
                tokio::task::yield_now().await;
            }
        })
        .await;
        let revision_while_blocked = last_applied_revision_id(environment);
        drop(engine);
        worker.await.unwrap();
        received.expect("refresh should reach the engine update");
        assert_eq!(revision_while_blocked, 1);
        assert_eq!(last_applied_revision_id(environment), 2);
        assert_eq!(
            refresher
                .engine_cache
                .get(&cache_key(&token))
                .unwrap()
                .get_state()
                .features[0]
                .name,
            "updated"
        );
    }

    #[derive(Clone)]
    struct StreamingServerState {
        request_count: Arc<AtomicUsize>,
        last_event_id_tx: Arc<Mutex<Option<oneshot::Sender<Option<String>>>>>,
    }

    async fn checkpoint_recovery_handler(
        State((state, lag)): State<(StreamingServerState, bool)>,
        request: Request,
    ) -> impl IntoResponse {
        let count = state.request_count.fetch_add(1, Ordering::SeqCst) + 1;
        let body = if count == 1 {
            let delta = serde_json::json!({"events":[{"type":"feature-updated","eventId":1,
                "feature":{"name":"flag","project":"default","enabled":false}}]});
            let checkpoint = serde_json::json!({"version":1,"revision":if lag {2} else {1},"stateHash":"a".repeat(64)});
            format!(
                "id: 1\nevent: unleash-updated\ndata: {delta}\n\nevent: unleash-state\ndata: {checkpoint}\n\n"
            )
        } else {
            let id = request
                .headers()
                .get("last-event-id")
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned);
            if let Some(tx) = state.last_event_id_tx.lock().await.take() {
                let _ = tx.send(id);
            }
            let delta = serde_json::json!({"events":[{"type":"hydration","eventId":2,
                "features":[{"name":"recovered","project":"default","enabled":true}],"segments":[]}]});
            format!("id: 2\nevent: unleash-connected\ndata: {delta}\n\n")
        };
        // Keep the connection open: only verification, not EOF, can reconnect it.
        let stream = futures::stream::once(async move { Ok::<_, Infallible>(body) })
            .chain(futures::stream::pending());
        Response::builder()
            .header(http::header::CONTENT_TYPE, "text/event-stream")
            .body(Body::from_stream(stream))
            .unwrap()
    }

    #[tokio::test]
    async fn shared_environment_sources_are_not_misreported_as_content_mismatches() {
        let (refresher, tokens) =
            build_delta_refresher_for_stream_test(Url::parse("http://localhost:4242").unwrap());
        let a = EdgeToken {
            token: "a".into(),
            projects: vec!["a".into()],
            environment: Some("production".into()),
            ..Default::default()
        };
        let b = EdgeToken {
            token: "b".into(),
            projects: vec!["b".into()],
            ..a.clone()
        };
        refresher
            .features_cache
            .insert("production".into(), ClientFeatures::default());
        tokens.insert(a.token.clone(), TokenRefresh::new(a.clone(), None));
        tokens.insert(b.token.clone(), TokenRefresh::new(b.clone(), None));
        assert!(refresher.state_fingerprint(&a).is_none());
        tokens.remove(&b.token);
        assert!(refresher.state_fingerprint(&a).is_some());
    }

    async fn assert_checkpoint_recovery(lag: bool) {
        let (tx, rx) = oneshot::channel();
        let state = StreamingServerState {
            request_count: Arc::new(AtomicUsize::new(0)),
            last_event_id_tx: Arc::new(Mutex::new(Some(tx))),
        };
        let server = TestServer::builder().http_transport().build(
            Router::new()
                .route("/api/client/streaming", get(checkpoint_recovery_handler))
                .with_state((state.clone(), lag)),
        );
        let (refresher, tokens) =
            build_delta_refresher_for_stream_test(server.server_url("/").unwrap());
        let token = EdgeToken::try_from("*:development.secret".to_string()).unwrap();
        tokens.insert(token.token.clone(), TokenRefresh::new(token.clone(), None));
        refresher.features_cache.insert(
            "development".into(),
            ClientFeatures {
                features: vec![ClientFeature {
                    name: "ghost".into(),
                    ..Default::default()
                }],
                ..Default::default()
            },
        );
        let (pause_tx, pause_rx) = watch::channel(RefreshState::Running);
        let join = tokio::spawn(super::run_stream_task_with_idle_timeout(
            refresher.clone(),
            token,
            server
                .server_url("/api/client/streaming")
                .unwrap()
                .to_string(),
            ClientMetaInformation::test_config(),
            vec![],
            pause_rx,
            super::SseIdleConfig {
                timeout: StdDuration::from_secs(90),
                reconnect_delay: StdDuration::ZERO,
                state_lag_threshold: Some(StdDuration::from_millis(10)),
            },
        ));
        assert_eq!(
            tokio::time::timeout(StdDuration::from_secs(5), rx)
                .await
                .unwrap()
                .unwrap(),
            None
        );
        tokio::time::timeout(StdDuration::from_secs(5), async {
            loop {
                if refresher
                    .features_cache
                    .get("development")
                    .is_some_and(|s| s.features.len() == 1 && s.features[0].name == "recovered")
                {
                    break;
                }
                tokio::time::sleep(StdDuration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(state.request_count.load(Ordering::SeqCst), 2);
        drop(pause_tx);
        tokio::time::timeout(StdDuration::from_secs(5), join)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn confirmed_mismatch_reconnects_without_cursor_and_hydrates() {
        assert_checkpoint_recovery(false).await;
    }

    #[tokio::test]
    async fn sustained_lag_reconnects_without_cursor_and_hydrates() {
        assert_checkpoint_recovery(true).await;
    }

    #[tokio::test]
    async fn scoped_hydration_preserves_other_projects_and_supports_empty_recovery() {
        let (refresher, _) =
            build_delta_refresher_for_stream_test(Url::parse("http://localhost:4242").unwrap());
        for project in ["a", "b", "a"] {
            let token = EdgeToken {
                projects: vec![project.into()],
                environment: Some("production".into()),
                ..Default::default()
            };
            let features = if project == "a"
                && refresher
                    .features_cache
                    .get("production")
                    .is_some_and(|s| s.features.len() == 2)
            {
                vec![]
            } else {
                vec![ClientFeature {
                    name: project.into(),
                    project: Some(project.into()),
                    ..Default::default()
                }]
            };
            refresher
                .handle_client_features_delta_updated(
                    &token,
                    ClientFeaturesDelta {
                        events: vec![DeltaEvent::Hydration {
                            event_id: 10,
                            features,
                            segments: vec![],
                        }],
                    },
                    None,
                )
                .await;
        }
        assert_eq!(
            refresher
                .features_cache
                .get("production")
                .unwrap()
                .features
                .iter()
                .map(|f| f.name.as_str())
                .collect::<Vec<_>>(),
            vec!["b"]
        );
        assert_eq!(
            refresher
                .delta_cache_manager
                .get("production")
                .unwrap()
                .get_hydration_event()
                .features[0]
                .name,
            "b"
        );
        let token = EdgeToken {
            projects: vec!["*".into()],
            environment: Some("production".into()),
            ..Default::default()
        };
        refresher
            .handle_client_features_delta_updated(
                &token,
                ClientFeaturesDelta {
                    events: vec![DeltaEvent::Hydration {
                        event_id: 11,
                        features: vec![],
                        segments: vec![],
                    }],
                },
                None,
            )
            .await;
        assert!(
            refresher
                .features_cache
                .get("production")
                .unwrap()
                .features
                .is_empty()
        );
    }

    async fn streaming_handler(
        State(state): State<StreamingServerState>,
        request: Request,
    ) -> impl IntoResponse {
        let count = state.request_count.fetch_add(1, Ordering::SeqCst) + 1;

        if count == 1 {
            let delta_json = serde_json::to_string(&revision(1)).unwrap();
            let sse_body = format!("id: 101\nevent: unleash-updated\ndata: {delta_json}\n\n");
            return Response::builder()
                .status(StatusCode::OK)
                .header(http::header::CONTENT_TYPE, "text/event-stream")
                .body(Body::from(sse_body))
                .unwrap();
        }

        let last_event_id = request
            .headers()
            .get("last-event-id")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string());

        if let Some(tx) = state.last_event_id_tx.lock().await.take() {
            let _ = tx.send(last_event_id);
        }

        Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .body(Body::empty())
            .unwrap()
    }

    #[tokio::test]
    async fn streaming_sends_last_event_id_on_reconnect() {
        let (server, last_event_id_rx) = streaming_test_server().await;
        let (delta_refresher, tokens_to_refresh) =
            build_delta_refresher_for_stream_test(server.server_url("/").unwrap());

        let token =
            EdgeToken::try_from("*:development.abcdefghijklmnopqrstuvwxyz".to_string()).unwrap();
        tokens_to_refresh.insert(token.token.clone(), TokenRefresh::new(token.clone(), None));

        let streaming_url = server
            .server_url("/api/client/streaming")
            .unwrap()
            .to_string();

        let (refresh_state_tx, refresh_state_rx) = watch::channel(RefreshState::Running);

        let join = tokio::spawn(super::run_stream_task(
            delta_refresher,
            token,
            streaming_url,
            ClientMetaInformation::test_config(),
            vec![],
            refresh_state_rx,
        ));

        let last_event_id = tokio::time::timeout(StdDuration::from_secs(5), last_event_id_rx)
            .await
            .expect("timed out waiting for last-event-id")
            .expect("last-event-id channel closed");

        assert_eq!(last_event_id.as_deref(), Some("101"));

        drop(refresh_state_tx);
        let _ = tokio::time::timeout(StdDuration::from_secs(5), join).await;
    }

    #[tokio::test]
    async fn streaming_reconnects_when_stream_goes_idle() {
        let (server, last_event_id_rx) = idle_streaming_test_server().await;
        let (delta_refresher, tokens_to_refresh) =
            build_delta_refresher_for_stream_test(server.server_url("/").unwrap());

        let token =
            EdgeToken::try_from("*:development.abcdefghijklmnopqrstuvwxyz".to_string()).unwrap();
        tokens_to_refresh.insert(token.token.clone(), TokenRefresh::new(token.clone(), None));

        let streaming_url = server
            .server_url("/api/client/streaming")
            .unwrap()
            .to_string();

        let (refresh_state_tx, refresh_state_rx) = watch::channel(RefreshState::Running);

        let join = tokio::spawn(super::run_stream_task_with_idle_timeout(
            delta_refresher,
            token,
            streaming_url,
            ClientMetaInformation::test_config(),
            vec![],
            refresh_state_rx,
            super::SseIdleConfig {
                timeout: StdDuration::from_millis(100),
                reconnect_delay: StdDuration::ZERO,
                state_lag_threshold: None,
            },
        ));

        let last_event_id = tokio::time::timeout(StdDuration::from_secs(5), last_event_id_rx)
            .await
            .expect("timed out waiting for reconnect after idle stream")
            .expect("last-event-id channel closed");

        assert_eq!(last_event_id.as_deref(), Some("101"));

        drop(refresh_state_tx);
        let _ = tokio::time::timeout(StdDuration::from_secs(5), join).await;
    }

    fn build_delta_refresher_for_stream_test(
        server_url: Url,
    ) -> (Arc<DeltaRefresher>, Arc<DashMap<String, TokenRefresh>>) {
        let unleash_client = build_unleash_client(server_url);
        let features_cache: Arc<FeatureCache> = Arc::new(FeatureCache::default());
        let delta_cache_manager: Arc<DeltaCacheManager> = Arc::new(DeltaCacheManager::new());
        let engine_cache: Arc<DashMap<String, EngineState>> = Arc::new(DashMap::default());
        let tokens_to_refresh = Arc::new(DashMap::default());

        let delta_refresher = Arc::new(DeltaRefresher {
            unleash_client,
            tokens_to_refresh: tokens_to_refresh.clone(),
            delta_cache_manager,
            features_cache,
            engine_cache,
            refresh_interval: Duration::seconds(6000),
            streaming: true,
            client_meta_information: ClientMetaInformation::test_config(),
            edge_instance_data: Arc::new(edge_instance_data_for_delta_refresher_test()),
        });

        (delta_refresher, tokens_to_refresh)
    }

    async fn streaming_test_server() -> (TestServer, oneshot::Receiver<Option<String>>) {
        let (last_event_id_tx, last_event_id_rx) = oneshot::channel();
        let state = StreamingServerState {
            request_count: Arc::new(AtomicUsize::new(0)),
            last_event_id_tx: Arc::new(Mutex::new(Some(last_event_id_tx))),
        };

        let router = Router::new()
            .route("/api/client/streaming", get(streaming_handler))
            .with_state(state);

        let server = TestServer::builder().http_transport().build(router);

        (server, last_event_id_rx)
    }

    async fn idle_streaming_handler(
        State(state): State<StreamingServerState>,
        request: Request,
    ) -> impl IntoResponse {
        let count = state.request_count.fetch_add(1, Ordering::SeqCst) + 1;

        if count == 1 {
            let delta_json = serde_json::to_string(&revision(1)).unwrap();
            let sse_body = format!("id: 101\nevent: unleash-updated\ndata: {delta_json}\n\n");
            let stream = futures::stream::once(async move { Ok::<_, Infallible>(sse_body) })
                .chain(futures::stream::pending());
            return Response::builder()
                .status(StatusCode::OK)
                .header(http::header::CONTENT_TYPE, "text/event-stream")
                .body(Body::from_stream(stream))
                .unwrap();
        }

        let last_event_id = request
            .headers()
            .get("last-event-id")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string());

        if let Some(tx) = state.last_event_id_tx.lock().await.take() {
            let _ = tx.send(last_event_id);
        }

        Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .body(Body::empty())
            .unwrap()
    }

    async fn idle_streaming_test_server() -> (TestServer, oneshot::Receiver<Option<String>>) {
        let (last_event_id_tx, last_event_id_rx) = oneshot::channel();
        let state = StreamingServerState {
            request_count: Arc::new(AtomicUsize::new(0)),
            last_event_id_tx: Arc::new(Mutex::new(Some(last_event_id_tx))),
        };

        let router = Router::new()
            .route("/api/client/streaming", get(idle_streaming_handler))
            .with_state(state);

        let server = TestServer::builder().http_transport().build(router);

        (server, last_event_id_rx)
    }

    fn cache_key(token: &EdgeToken) -> String {
        token
            .environment
            .clone()
            .unwrap_or_else(|| token.token.clone())
    }

    fn hydration_delta(event_id: u32, feature_names: &[&str]) -> ClientFeaturesDelta {
        ClientFeaturesDelta {
            events: vec![DeltaEvent::Hydration {
                event_id,
                features: feature_names.iter().map(|name| feature(name)).collect(),
                segments: vec![],
            }],
        }
    }

    fn feature_updated_event(event_id: u32, feature_name: &str) -> DeltaEvent {
        DeltaEvent::FeatureUpdated {
            event_id,
            feature: feature(feature_name),
        }
    }

    fn feature(name: &str) -> ClientFeature {
        ClientFeature {
            name: name.into(),
            feature_type: Some("release".into()),
            ..Default::default()
        }
    }

    fn feature_with_invalid_single_value_constraint(name: &str) -> ClientFeature {
        ClientFeature {
            name: name.into(),
            enabled: true,
            strategies: Some(vec![Strategy {
                name: "default".into(),
                parameters: None,
                sort_order: None,
                segments: None,
                constraints: Some(vec![Constraint {
                    context_name: "userId".into(),
                    operator: Operator::NumEq,
                    case_insensitive: false,
                    inverted: false,
                    values: None,
                    value: None,
                }]),
                variants: None,
            }]),
            ..Default::default()
        }
    }

    fn delta_with_invalid_single_value_constraint(event_id: u32) -> ClientFeaturesDelta {
        ClientFeaturesDelta {
            events: vec![DeltaEvent::FeatureUpdated {
                event_id,
                feature: feature_with_invalid_single_value_constraint("invalid-constraint-flag"),
            }],
        }
    }

    fn revision(revision_id: u32) -> ClientFeaturesDelta {
        match revision_id {
            1 => ClientFeaturesDelta {
                events: vec![
                    DeltaEvent::FeatureUpdated {
                        event_id: 1,
                        feature: ClientFeature {
                            name: "test1".into(),
                            feature_type: Some("release".into()),
                            ..Default::default()
                        },
                    },
                    DeltaEvent::FeatureUpdated {
                        event_id: 1,
                        feature: ClientFeature {
                            name: "test2".into(),
                            feature_type: Some("release".into()),
                            ..Default::default()
                        },
                    },
                    DeltaEvent::SegmentUpdated {
                        event_id: 1,
                        segment: Segment {
                            id: 1,
                            constraints: vec![Constraint {
                                context_name: "userId".into(),
                                operator: Operator::In,
                                case_insensitive: false,
                                inverted: false,
                                values: Some(vec!["7".into()]),
                                value: None,
                            }],
                        },
                    },
                ],
            },
            _ => ClientFeaturesDelta {
                events: vec![
                    DeltaEvent::FeatureUpdated {
                        event_id: 2,
                        feature: ClientFeature {
                            name: "test1".into(),
                            feature_type: Some("release".into()),
                            ..Default::default()
                        },
                    },
                    DeltaEvent::FeatureRemoved {
                        event_id: 2,
                        feature_name: "test2".to_string(),
                        project: "default".to_string(),
                    },
                ],
            },
        }
    }

    async fn return_client_features_delta(etag_header: Option<String>) -> impl IntoResponse {
        match etag_header {
            Some(value) => match value.as_str() {
                "\"1\"" => Response::builder()
                    .status(StatusCode::OK)
                    .header(
                        http::header::ETAG,
                        entity_tag_to_header_value(EntityTag::new(false, "2")),
                    )
                    .body(Body::from(serde_json::to_vec(&revision(2)).unwrap()))
                    .unwrap(),
                "\"2\"" => Response::builder()
                    .status(StatusCode::NOT_MODIFIED)
                    .body(Body::empty())
                    .unwrap(),
                _ => Response::builder()
                    .status(StatusCode::NOT_MODIFIED)
                    .body(Body::empty())
                    .unwrap(),
            },
            None => Response::builder()
                .status(StatusCode::OK)
                .header(
                    http::header::ETAG,
                    entity_tag_to_header_value(EntityTag::new(false, "1")),
                )
                .body(Body::from(serde_json::to_vec(&revision(1)).unwrap()))
                .unwrap(),
        }
    }

    async fn test_features_server() -> TestServer {
        let router = Router::new().route("/api/client/delta", get(delta_handler));
        TestServer::builder().http_transport().build(router)
    }

    async fn delta_handler(request: Request) -> impl IntoResponse {
        let etag_header = request
            .headers()
            .get(http::header::IF_NONE_MATCH)
            .and_then(|h| h.to_str().ok());
        return_client_features_delta(etag_header.map(|s| s.to_string())).await
    }
}
