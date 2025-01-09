mod delta_test {
    use actix_http::header::IF_NONE_MATCH;
    use actix_http::HttpService;
    use actix_http_test::{test_server, TestServer};
    use actix_service::map_config;
    use actix_web::dev::AppConfig;
    use actix_web::http::header::{ETag, EntityTag};
    use actix_web::{web, App, HttpRequest, HttpResponse};
    use chrono::Duration;
    use dashmap::DashMap;
    use std::sync::Arc;
    use unleash_edge::feature_cache::FeatureCache;
    use unleash_edge::http::feature_refresher::FeatureRefresher;
    use unleash_edge::http::unleash_client::UnleashClient;
    use unleash_edge::types::EdgeToken;
    use unleash_types::client_features::{
        ClientFeature, ClientFeatures, ClientFeaturesDelta, Constraint, Operator, Segment,
    };
    use unleash_yggdrasil::EngineState;

    #[actix_web::test]
    #[tracing_test::traced_test]
    async fn test_delta() {
        let srv = test_features_server().await;
        let unleash_client = Arc::new(UnleashClient::new(srv.url("/").as_str(), None).unwrap());
        let features_cache: Arc<FeatureCache> = Arc::new(FeatureCache::default());
        let engine_cache: Arc<DashMap<String, EngineState>> = Arc::new(DashMap::default());

        let feature_refresher = Arc::new(FeatureRefresher {
            unleash_client: unleash_client.clone(),
            tokens_to_refresh: Arc::new(Default::default()),
            features_cache: features_cache.clone(),
            engine_cache: engine_cache.clone(),
            refresh_interval: Duration::seconds(6000),
            persistence: None,
            strict: false,
            streaming: false,
            delta: true,
            app_name: "test-app".into(),
        });
        let features = ClientFeatures {
            version: 1,
            features: vec![],
            segments: None,
            query: None,
            meta: None,
        };
        let initial_features = features.modify_and_copy(&revision(1));
        let final_features = initial_features.modify_and_copy(&revision(2));
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
        assert_eq!(refreshed_features, initial_features);

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
        assert_eq!(refreshed_features, final_features);
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
                updated: vec![
                    ClientFeature {
                        name: "test1".into(),
                        feature_type: Some("release".into()),
                        ..Default::default()
                    },
                    ClientFeature {
                        name: "test2".into(),
                        feature_type: Some("release".into()),
                        ..Default::default()
                    },
                ],
                removed: vec![],
                segments: Some(vec![Segment {
                    id: 1,
                    constraints: vec![Constraint {
                        context_name: "userId".into(),
                        operator: Operator::In,
                        case_insensitive: false,
                        inverted: false,
                        values: Some(vec!["7".into()]),
                        value: None,
                    }],
                }]),
                revision_id: 1,
            },
            _ => ClientFeaturesDelta {
                updated: vec![ClientFeature {
                    name: "test1".into(),
                    feature_type: Some("release".into()),
                    ..Default::default()
                }],
                removed: vec!["test2".to_string()],
                segments: None,
                revision_id: 2,
            },
        }
    }

    async fn return_client_features_delta(etag_header: Option<String>) -> HttpResponse {
        match etag_header {
            Some(value) => match value.as_str() {
                "\"1\"" => HttpResponse::Ok()
                    .insert_header(ETag(EntityTag::new_strong("2".to_string())))
                    .json(revision(2)),
                "\"2\"" => HttpResponse::NotModified().finish(),
                _ => HttpResponse::NotModified().finish(),
            },
            None => HttpResponse::Ok()
                .insert_header(ETag(EntityTag::new_strong("1".to_string())))
                .json(revision(1)),
        }
    }

    async fn test_features_server() -> TestServer {
        test_server(move || {
            HttpService::new(map_config(
                App::new().service(web::resource("/api/client/delta").route(web::get().to(
                    |req: HttpRequest| {
                        println!("Got delta request");
                        let etag_header = req
                            .headers()
                            .get(IF_NONE_MATCH)
                            .and_then(|h| h.to_str().ok());
                        println!("Our etag header is {etag_header:?}");
                        return_client_features_delta(etag_header.map(|s| s.to_string()))
                    },
                ))),
                |_| AppConfig::default(),
            ))
            .tcp()
        })
        .await
    }
}
