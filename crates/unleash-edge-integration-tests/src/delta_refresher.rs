#[cfg(test)]
mod tests {
    use chrono::Duration;
    use dashmap::DashMap;
    use std::sync::Arc;
    use axum::body::Body;
    use axum::extract::Request;
    use axum::{http, Router};
    use axum::http::{StatusCode};
    use axum::http::header::ETAG;
    use axum::response::Response;
    use axum::routing::get;
    use axum_test::TestServer;
    use etag::EntityTag;
    use tracing::info;
    use unleash_types::client_features::{
        ClientFeature, ClientFeatures, ClientFeaturesDelta, Constraint, DeltaEvent, Operator,
        Segment,
    };
    use unleash_yggdrasil::EngineState;
    use unleash_edge_delta::cache_manager::DeltaCacheManager;
    use unleash_edge_feature_cache::FeatureCache;
    use unleash_edge_feature_refresh::FeatureRefresher;
    use unleash_edge_http_client::{ClientMetaInformation, UnleashClient};
    use unleash_edge_types::{EdgeResult, EngineCache};
    use unleash_edge_types::tokens::EdgeToken;

    #[tokio::test]
    #[tracing_test::traced_test]
    async fn test_delta() {
        let srv = test_features_server().await;
        let unleash_client = Arc::new(UnleashClient::from_url(srv.server_url("/").unwrap(), None).unwrap());
        let features_cache: Arc<FeatureCache> = Arc::new(FeatureCache::default());
        let delta_cache_manager: Arc<DeltaCacheManager> = Arc::new(DeltaCacheManager::new());
        let engine_cache: Arc<EngineCache> = Arc::new(DashMap::default());

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

    async fn return_client_features_delta(etag_header: Option<String>) -> Response {
        match etag_header {
            Some(value) => match value.as_str() {
                "\"1\"" => Response::builder().status(StatusCode::OK).header(ETAG, EntityTag::new(false, "2").to_string())
                    .body(Body::from(serde_json::to_vec(&revision(2)).unwrap())).unwrap(),
                "\"2\"" => Response::builder().status(StatusCode::NOT_MODIFIED).body(Body::empty()).unwrap(),
                _ => Response::builder().status(StatusCode::NOT_MODIFIED).body(Body::empty()).unwrap(),
            },
            None => Response::builder().status(StatusCode::OK).header(ETAG, EntityTag::new(false, "1").to_string())
                .body(Body::from(serde_json::to_vec(&revision(1)).unwrap())).unwrap()
        }
    }

    async fn handle_delta(req: Request) -> Response {
        let etag_header = req
            .headers()
            .get(http::header::IF_NONE_MATCH)
            .and_then(|h| h.to_str().ok());
        return_client_features_delta(etag_header.map(|s| s.to_string())).await
    }

    async fn test_features_server() -> TestServer {
        let router = Router::new()
            .route("/api/client/delta", get(handle_delta));
        TestServer::builder().http_transport().build(router).unwrap()
    }
}