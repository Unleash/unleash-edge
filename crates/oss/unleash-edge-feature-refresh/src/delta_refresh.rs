use anyhow::Context;
use dashmap::DashMap;
use etag::EntityTag;
use eventsource_client::Client;
use futures::StreamExt;
use reqwest::StatusCode;
use std::collections::HashSet;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch::Receiver;
use tokio::time::sleep;
use tracing::{debug, info, warn};
use unleash_edge_delta::cache::{DeltaCache, DeltaHydrationEvent};
use unleash_edge_delta::cache_manager::DeltaCacheManager;
use unleash_edge_feature_cache::FeatureCache;
use unleash_edge_feature_filters::FeatureFilterSet;
use unleash_edge_feature_filters::delta_filters::{DeltaFilterSet, filter_delta_events};
use unleash_edge_http_client::{ClientMetaInformation, UnleashClient};
use unleash_edge_persistence::EdgePersistence;
use unleash_edge_types::errors::{EdgeError, FeatureError};
use unleash_edge_types::headers::{
    UNLEASH_APPNAME_HEADER, UNLEASH_CLIENT_SPEC_HEADER, UNLEASH_CONNECTION_ID_HEADER,
    UNLEASH_INSTANCE_ID_HEADER,
};
use unleash_edge_types::tokens::{EdgeToken, cache_key, simplify};
use unleash_edge_types::{
    ClientFeaturesDeltaResponse, ClientFeaturesRequest, EdgeResult, RefreshState, TokenRefresh,
};
use unleash_types::client_features::{ClientFeaturesDelta, DeltaEvent};
use unleash_yggdrasil::EngineState;

use crate::{TokenRefreshSet, TokenRefreshStatus, client_application_from_token_and_name};

pub type Environment = String;

const DELTA_CACHE_LIMIT: usize = 100;

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
) -> anyhow::Result<SseStream> {
    let mut es_client_builder = eventsource_client::ClientBuilder::for_url(streaming_url)
        .context("Failed to create EventSource client for streaming")?
        .header("Authorization", &token.token)?
        .header(UNLEASH_APPNAME_HEADER, &client_meta_information.app_name)?
        .header(
            UNLEASH_INSTANCE_ID_HEADER,
            &client_meta_information.instance_id.to_string(),
        )?
        .header(
            UNLEASH_CONNECTION_ID_HEADER,
            &client_meta_information.connection_id.to_string(),
        )?
        .header(
            UNLEASH_CLIENT_SPEC_HEADER,
            unleash_yggdrasil::SUPPORTED_SPEC_VERSION,
        )?;

    for (key, value) in custom_headers {
        es_client_builder = es_client_builder.header(key, value)?;
    }

    let client = es_client_builder.reconnect(reconnect_opts()).build();
    Ok(client.stream())
}

async fn handle_sse(
    sse: eventsource_client::SSE,
    delta_refresher: &Arc<DeltaRefresher>,
    token: &EdgeToken,
) {
    match sse {
        eventsource_client::SSE::Event(event)
            if event.event_type == "unleash-connected" || event.event_type == "unleash-updated" =>
        {
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
                }
                Err(e) => {
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
}

async fn run_stream_task(
    delta_refresher: Arc<DeltaRefresher>,
    token: EdgeToken,
    streaming_url: String,
    client_meta_information: ClientMetaInformation,
    custom_headers: Vec<(String, String)>,
    mut refresh_state_rx: Receiver<RefreshState>,
) {
    let mut stream: Option<SseStream> = None;

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
            ) {
                Ok(s) => s,
                Err(e) => {
                    warn!(
                        "SSE misconfiguration detected; cannot build stream: {e:?}. Exiting stream task."
                    );
                    return;
                }
            };

            stream = Some(s);
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
                        sleep(Duration::from_secs(1)).await;
                    }
                }
                next = s.next() => {
                    match next {
                        Some(Ok(sse)) => {
                            handle_sse(sse, &delta_refresher, &token).await;
                        }
                        Some(Err(e)) => {
                            info!("SSE stream error: {e:?}; reconnecting immediately");
                            stream = None;
                            continue;
                        }
                        None => {
                            info!("SSE stream ended; reconnecting immediately");
                            stream = None;
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
    pub persistence: Option<Arc<dyn EdgePersistence>>,
    pub streaming: bool,
    pub client_meta_information: ClientMetaInformation,
}

impl DeltaRefresher {
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
        delta: ClientFeaturesDelta,
        etag: Option<EntityTag>,
    ) {
        let updated_len = delta.events.len();

        debug!(
            "Got updated client features delta. Updating features with etag {etag:?}, events count {updated_len}"
        );

        let key: String = cache_key(refresh_token);
        self.features_cache.apply_delta(key.clone(), &delta);

        if let Some(mut _entry) = self.delta_cache_manager.get(&key) {
            self.delta_cache_manager.update_cache(&key, &delta.events);
        } else if let Some(DeltaEvent::Hydration {
            event_id,
            features,
            segments,
        }) = delta.events.clone().into_iter().next()
        {
            self.delta_cache_manager.insert_cache(
                &key,
                DeltaCache::new(
                    DeltaHydrationEvent {
                        event_id,
                        features,
                        segments,
                    },
                    DELTA_CACHE_LIMIT,
                ),
            );
        } else {
            warn!(
                "Warning: No hydrationEvent found in delta.events, but cache empty for environment"
            );
        }

        self.tokens_to_refresh.update_last_refresh(
            refresh_token,
            etag,
            self.features_cache.get(&key).unwrap().features.len(),
            &self.refresh_interval,
        );
        self.engine_cache
            .entry(key.clone())
            .and_modify(|engine| {
                engine.apply_delta(&delta);
            })
            .or_insert_with(|| {
                let mut new_state = EngineState::default();

                let warnings = new_state.apply_delta(&delta);
                if let Some(warnings) = warnings {
                    warn!("The following toggle failed to compile and will be defaulted to off: {warnings:?}");
                };
                new_state
            });
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
    use axum::Router;
    use axum::body::Body;
    use axum::extract::Request;
    use axum::response::{IntoResponse, Response};
    use axum::routing::get;
    use axum_test::TestServer;
    use chrono::Duration;
    use dashmap::DashMap;
    use etag::EntityTag;
    use http::StatusCode;
    use reqwest::Url;
    use std::sync::Arc;
    use ulid::Ulid;
    use unleash_edge_delta::cache_manager::DeltaCacheManager;
    use unleash_edge_feature_cache::FeatureCache;
    use unleash_edge_http_client::{
        ClientMetaInformation, HttpClientArgs, UnleashClient, new_reqwest_client,
    };
    use unleash_edge_types::entity_tag_to_header_value;
    use unleash_edge_types::tokens::EdgeToken;
    use unleash_types::client_features::{
        ClientFeature, ClientFeatures, ClientFeaturesDelta, Constraint, DeltaEvent, Operator,
        Segment,
    };
    use unleash_yggdrasil::EngineState;

    trait TestConfig {
        fn test_config() -> Self;
    }

    impl TestConfig for ClientMetaInformation {
        fn test_config() -> Self {
            ClientMetaInformation {
                app_name: "test_app".into(),
                instance_id: Ulid::new(),
                connection_id: Ulid::new(),
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
            persistence: None,
            streaming: false,
            client_meta_information: ClientMetaInformation::test_config(),
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

    fn cache_key(token: &EdgeToken) -> String {
        token
            .environment
            .clone()
            .unwrap_or_else(|| token.token.clone())
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
        TestServer::builder()
            .http_transport()
            .build(router)
            .unwrap()
    }

    async fn delta_handler(request: Request) -> impl IntoResponse {
        let etag_header = request
            .headers()
            .get(http::header::IF_NONE_MATCH)
            .and_then(|h| h.to_str().ok());
        return_client_features_delta(etag_header.map(|s| s.to_string())).await
    }
}
