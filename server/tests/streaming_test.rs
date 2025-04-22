mod streaming_test {
    use dashmap::DashMap;
    use eventsource_client::Client;
    use futures::StreamExt;
    use std::{
        process::Command,
        str::FromStr,
        sync::Arc,
    };
    use unleash_edge::{
        cli::{EdgeArgs, EdgeMode, TokenHeader},
        feature_cache::FeatureCache,
        http::broadcaster::Broadcaster,
        tokens::cache_key,
        types::{EdgeToken, TokenType, TokenValidationStatus},
    };
    use unleash_types::client_features::{ClientFeature, ClientFeaturesDelta, DeltaEvent};

    #[actix_web::test]
    async fn test_streaming() {
        let unleash_features_cache: Arc<FeatureCache> =
            Arc::new(FeatureCache::new(DashMap::default()));
        let delta_cache_manager: Arc<DeltaCacheManager> = Arc::new(DeltaCacheManager::new());
        let unleash_token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());
        let unleash_broadcaster = Broadcaster::new(delta_cache_manager.clone());

        let unleash_server = upstream_server(
            unleash_token_cache.clone(),
            unleash_features_cache.clone(),
            delta_cache_manager.clone(),
            Arc::new(DashMap::default()),
            unleash_broadcaster.clone(),
        )
        .await;

        let mut upstream_known_token = EdgeToken::from_str("dx:development.secret123").unwrap();
        upstream_known_token.status = TokenValidationStatus::Validated;
        upstream_known_token.token_type = Some(TokenType::Client);
        unleash_token_cache.insert(
            upstream_known_token.token.clone(),
            upstream_known_token.clone(),
        );

        let delta_cache = DeltaCache::new(
            DeltaHydrationEvent {
                event_id: 1,
                features: vec![ClientFeature {
                    name: "feature1".to_string(),
                    project: Some("dx".to_string()),
                    enabled: false,
                    ..Default::default()
                }],
                segments: vec![],
            },
            10,
        );
        delta_cache_manager.insert_cache(&cache_key(&upstream_known_token), delta_cache);

        let mut edge = Command::new("./../target/debug/unleash-edge")
            .arg("edge")
            .arg("--upstream-url")
            .arg(unleash_server.url("/"))
            .arg("--strict")
            .arg("--streaming")
            .arg("--delta")
            .arg("-t")
            .arg(&upstream_known_token.token)
            .spawn()
            .expect("Failed to start the app");

        // Allow edge to establish a connection with upstream and populate the cache
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        // let es_client = eventsource_client::ClientBuilder::for_url(&edge.url("/api/client/streaming"))
        let es_client = eventsource_client::ClientBuilder::for_url(
            "http://localhost:3063/api/client/streaming",
        )
        .unwrap()
        .header("Authorization", &upstream_known_token.token)
        .unwrap()
        .build();

        let initial_event = ClientFeaturesDelta {
            events: vec![DeltaEvent::Hydration {
                event_id: 1,
                features: vec![ClientFeature {
                    name: "feature1".to_string(),
                    project: Some("dx".to_string()),
                    enabled: false,
                    ..Default::default()
                }],
                segments: vec![],
            }],
        };

        let mut stream = es_client.stream();

        if tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                let next = stream.next().await;
                if let Some(Ok(event)) = next {
                    match event {
                        eventsource_client::SSE::Event(event)
                            if event.event_type == "unleash-connected" =>
                        {
                            assert_eq!(
                                serde_json::from_str::<ClientFeaturesDelta>(&event.data).unwrap(),
                                initial_event
                            );
                            println!("unleash-connected event received; features match expected");
                            break;
                        }
                        e => {
                            println!("Other event received; ignoring {:#?}", e);
                        }
                    }
                } else if let Some(error) = next {
                    error!("{:#?}", error);
                }
            }
        })
        .await
        .is_err()
        {
            // If the test times out, kill the app process and fail the test
            edge.kill().expect("Failed to kill the app process");
            edge.wait().expect("Failed to wait for the app process");
            panic!("Test timed out waiting for unleash-connected event");
        }

        let update_events = vec![DeltaEvent::FeatureUpdated {
            event_id: 3,
            feature: ClientFeature {
                name: "feature1".to_string(),
                project: Some("dx".to_string()),
                enabled: true,
                ..Default::default()
            },
        }];
        delta_cache_manager.update_cache(&cache_key(&upstream_known_token), &update_events);

        if tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                let next = stream.next().await;
                if let Some(Ok(event)) = next {
                    match event {
                        eventsource_client::SSE::Event(event)
                            if event.event_type == "unleash-updated" =>
                        {
                            assert_eq!(
                                serde_json::from_str::<ClientFeaturesDelta>(&event.data).unwrap(),
                                ClientFeaturesDelta {
                                    events: update_events
                                }
                            );
                            println!("unleash-updated event received;");
                            break;
                        }
                        e => {
                            println!("Other event received; ignoring {:#?}", e);
                        }
                    }
                } else if let Some(error) = next {
                    error!("{:#?}", error);
                }
            }
        })
        .await
        .is_err()
        {
            // If the test times out, kill the app process and fail the test
            edge.kill().expect("Failed to kill the app process");
            edge.wait().expect("Failed to wait for the app process");
            panic!("Test timed out waiting for unleash-updated event");
        }

        edge.kill().expect("Failed to kill the app process");
        edge.wait().expect("Failed to wait for the app process");
    }

    use actix_http::HttpService;
    use actix_http_test::{TestServer, test_server};
    use actix_service::map_config;
    use actix_web::dev::AppConfig;
    use actix_web::{App, web};
    use tracing::error;
    use unleash_types::client_metrics::ConnectVia;
    use unleash_yggdrasil::EngineState;

    use unleash_edge::auth::token_validator::TokenValidator;
    use unleash_edge::delta_cache::{DeltaCache, DeltaHydrationEvent};
    use unleash_edge::delta_cache_manager::DeltaCacheManager;
    use unleash_edge::metrics::client_metrics::MetricsCache;

    async fn upstream_server(
        upstream_token_cache: Arc<DashMap<String, EdgeToken>>,
        upstream_features_cache: Arc<FeatureCache>,
        upstream_delta_cache_manager: Arc<DeltaCacheManager>,
        upstream_engine_cache: Arc<DashMap<String, EngineState>>,
        upstream_broadcaster: Arc<Broadcaster>,
    ) -> TestServer {
        let token_validator = Arc::new(TokenValidator {
            unleash_client: Arc::new(Default::default()),
            token_cache: upstream_token_cache.clone(),
            persistence: None,
        });

        test_server(move || {
            // the streaming endpoint doesn't work unless app data contains an EdgeMode::Edge with streaming: true
            let edge_mode = EdgeMode::Edge(EdgeArgs {
                streaming: true,
                upstream_url: "".into(),
                backup_folder: None,
                metrics_interval_seconds: 60,
                features_refresh_interval_seconds: 60,
                token_revalidation_interval_seconds: 60,
                tokens: vec!["".into()],
                custom_client_headers: vec![],
                skip_ssl_verification: false,
                client_identity: None,
                upstream_certificate_file: None,
                upstream_request_timeout: 5,
                upstream_socket_timeout: 5,
                redis: None,
                s3: None,
                token_header: TokenHeader {
                    token_header: "".into(),
                },
                strict: true,
                dynamic: false,
                delta: false,
                delta_diff: false,
                consumption: false,
                prometheus_remote_write_url: None,
                prometheus_push_interval: 60,
                prometheus_username: None,
                prometheus_password: None,
                prometheus_user_id: None,
            });

            let config = serde_qs::actix::QsQueryConfig::default()
                .qs_config(serde_qs::Config::new(5, false));
            let metrics_cache = MetricsCache::default();
            let connect_via = ConnectVia {
                app_name: "edge".into(),
                instance_id: "testinstance".into(),
            };
            HttpService::new(map_config(
                App::new()
                    .app_data(config)
                    .app_data(web::Data::from(token_validator.clone()))
                    .app_data(web::Data::from(upstream_features_cache.clone()))
                    .app_data(web::Data::from(upstream_delta_cache_manager.clone()))
                    .app_data(web::Data::from(upstream_broadcaster.clone()))
                    .app_data(web::Data::from(upstream_engine_cache.clone()))
                    .app_data(web::Data::from(upstream_token_cache.clone()))
                    .app_data(web::Data::new(metrics_cache))
                    .app_data(web::Data::new(connect_via))
                    .app_data(web::Data::new(edge_mode))
                    .service(
                        web::scope("/api")
                            .configure(unleash_edge::client_api::configure_client_api)
                            .configure(|cfg| {
                                unleash_edge::frontend_api::configure_frontend_api(cfg, false)
                            }),
                    )
                    .service(
                        web::scope("/edge").configure(unleash_edge::edge_api::configure_edge_api),
                    ),
                |_| AppConfig::default(),
            ))
            .tcp()
        })
        .await
    }
}
