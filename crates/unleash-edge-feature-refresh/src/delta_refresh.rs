use eventsource_client::Client;
use futures::TryStreamExt;
use reqwest::StatusCode;
use std::time::Duration;
use etag::EntityTag;
use tracing::{debug, info, warn};
use unleash_types::client_features::{ClientFeaturesDelta, DeltaEvent};
use unleash_yggdrasil::EngineState;
use unleash_edge_delta::cache::{DeltaCache, DeltaHydrationEvent};
use unleash_edge_http_client::ClientMetaInformation;
use unleash_edge_types::{ClientFeaturesDeltaResponse, ClientFeaturesRequest, TokenRefresh};
use unleash_edge_types::errors::{EdgeError, FeatureError};
use unleash_edge_types::headers::{UNLEASH_APPNAME_HEADER, UNLEASH_CLIENT_SPEC_HEADER, UNLEASH_CONNECTION_ID_HEADER, UNLEASH_INSTANCE_ID_HEADER};
use unleash_edge_types::tokens::{cache_key, EdgeToken};
use crate::FeatureRefresher;

pub type Environment = String;

const DELTA_CACHE_LIMIT: usize = 100;

impl FeatureRefresher {
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

        self.update_last_refresh(
            refresh_token,
            etag,
            self.features_cache.get(&key).unwrap().features.len(),
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
                    self.update_last_check(&refresh.token.clone());
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

    pub async fn start_streaming_delta_background_task(
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
                                            Ok(delta) => { refresher.handle_client_features_delta_updated(&token, delta, None).await; }
                                            Err(e) => { warn!("Could not parse features response to internal representation: {e:?}");
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
                                            Ok(delta) => { refresher.handle_client_features_delta_updated(&token, delta, None).await; }
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
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use axum::body::Body;
    use axum::extract::Request;
    use axum::response::{IntoResponse, Response};
    use axum::Router;
    use axum::routing::get;
    use axum_test::TestServer;
    use chrono::Duration;
    use dashmap::DashMap;
    use etag::EntityTag;
    use http::StatusCode;
    use unleash_types::client_features::{ClientFeature, ClientFeatures, ClientFeaturesDelta, Constraint, DeltaEvent, Operator, Segment};
    use unleash_yggdrasil::EngineState;
    use unleash_edge_delta::cache_manager::DeltaCacheManager;
    use unleash_edge_feature_cache::FeatureCache;
    use unleash_edge_http_client::{ClientMetaInformation, UnleashClient};
    use unleash_edge_types::entity_tag_to_header_value;
    use unleash_edge_types::tokens::EdgeToken;
    use crate::FeatureRefresher;

    #[tokio::test]
    #[tracing_test::traced_test]
    async fn test_delta() {
        let srv = test_features_server().await;
        let unleash_client = Arc::new(UnleashClient::new(srv.server_url("/").unwrap().as_str(), None).unwrap());
        let features_cache: Arc<FeatureCache> = Arc::new(FeatureCache::default());
        let delta_cache_manager: Arc<DeltaCacheManager> = Arc::new(DeltaCacheManager::new());
        let engine_cache: Arc<DashMap<String, EngineState>> = Arc::new(DashMap::default());

        let feature_refresher = Arc::new(FeatureRefresher {
            unleash_client: unleash_client.clone(),
            tokens_to_refresh: Arc::new(Default::default()),
            delta_cache_manager,
            features_cache: features_cache.clone(),
            engine_cache: engine_cache.clone(),
            refresh_interval: Duration::seconds(6000),
            persistence: None,
            strict: false,
            streaming: false,
            delta: true,
            delta_diff: false,
            client_meta_information: ClientMetaInformation::test_config(),
        });
        let mut delta_features = ClientFeatures::create_from_delta(&revision(1));
        let token =
            EdgeToken::try_from("*:development.abcdefghijklmnopqrstuvwxyz".to_string()).unwrap();
        feature_refresher
            .register_token_for_refresh(token.clone(), None)
            .await;
        feature_refresher.refresh_features().await;
        let refreshed_features = features_cache
            .get(&cache_key(&token))
            .unwrap()
            .value()
            .clone();
        assert_eq!(refreshed_features, delta_features);

        let token_refresh = feature_refresher
            .tokens_to_refresh
            .get(&token.token)
            .unwrap()
            .clone();
        feature_refresher.refresh_single_delta(token_refresh).await;
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
                "\"1\"" => Response::builder().status(StatusCode::OK)
                    .header(http::header::ETAG, entity_tag_to_header_value(EntityTag::new(false, "2")))
                    .body(Body::from(serde_json::to_vec(&revision(2)).unwrap())).unwrap(),
                "\"2\"" => Response::builder().status(StatusCode::NOT_MODIFIED).body(Body::empty()).unwrap(),
                _ => Response::builder().status(StatusCode::NOT_MODIFIED).body(Body::empty()).unwrap(),
            },
            None => Response::builder()
                .status(StatusCode::OK)
                .header(http::header::ETAG, entity_tag_to_header_value(EntityTag::new(false, "1")))
                .body(Body::from(serde_json::to_vec(&revision(1)).unwrap())).unwrap(),
        }
    }

    async fn test_features_server() -> TestServer {
        let router = Router::new()
            .route("/api/client/delta", get(delta_handler));
        TestServer::builder().http_transport().build(router).unwrap()
    }

    async fn delta_handler(request: Request) -> impl IntoResponse {
        let etag_header = request.headers().get(http::header::IF_NONE_MATCH).and_then(|h| h.to_str().ok());
        return_client_features_delta(etag_header.map(|s| s.to_string())).await
    }
}
