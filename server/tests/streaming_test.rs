mod streaming_test {
    use dashmap::DashMap;
    use eventsource_client::Client;
    use futures::StreamExt;
    use std::{
        fs,
        io::BufReader,
        path::PathBuf,
        process::{Command, Stdio},
        str::FromStr,
        sync::Arc,
    };
    use unleash_edge::{
        feature_cache::FeatureCache,
        http::broadcaster::Broadcaster,
        tokens::cache_key,
        types::{EdgeToken, TokenType, TokenValidationStatus},
    };
    use unleash_types::client_features::{ClientFeatures, Query};

    pub fn features_from_disk(path: &str) -> ClientFeatures {
        let path = PathBuf::from(path);
        let file = fs::File::open(path).unwrap();
        let reader = BufReader::new(file);
        serde_json::from_reader(reader).unwrap()
    }

    #[actix_web::test]
    async fn test_streaming() {
        let unleash_features_cache: Arc<FeatureCache> =
            Arc::new(FeatureCache::new(DashMap::default()));
        let unleash_token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());
        let unleash_broadcaster = Broadcaster::new(unleash_features_cache.clone());

        let unleash_server = upstream_server(
            unleash_token_cache.clone(),
            unleash_features_cache.clone(),
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

        unleash_features_cache.insert(
            cache_key(&upstream_known_token),
            features_from_disk("../examples/features.json"),
        );

        let mut edge = Command::new("./../target/debug/unleash-edge")
            .arg("edge")
            .arg("--upstream-url")
            .arg(unleash_server.url("/"))
            .arg("--strict")
            .arg("--streaming")
            .arg("-t")
            .arg(&upstream_known_token.token)
            .stdout(Stdio::null()) // Suppress stdout
            .stderr(Stdio::null()) // Suppress stderr
            .spawn()
            .expect("Failed to start the app");

        // Allow edge to establish a connection with upstream and populate the cache
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        // let es_client = eventsource_client::ClientBuilder::for_url(&edge.url("/api/client/streaming"))
        let es_client = eventsource_client::ClientBuilder::for_url(
            "http://localhost:3063/api/client/streaming",
        )
        .unwrap()
        .header("Authorization", &upstream_known_token.token)
        .unwrap()
        .build();

        let initial_features = ClientFeatures {
            features: vec![],
            version: 2,
            segments: None,
            query: Some(Query {
                tags: None,
                projects: Some(vec!["dx".into()]),
                name_prefix: None,
                environment: Some("development".into()),
                inline_segment_constraints: Some(false),
            }),
        };

        let mut stream = es_client.stream();

        if tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if let Some(Ok(event)) = stream.next().await {
                    match event {
                        eventsource_client::SSE::Event(event)
                            if event.event_type == "unleash-connected" =>
                        {
                            assert_eq!(
                                serde_json::from_str::<ClientFeatures>(&event.data).unwrap(),
                                initial_features
                            );
                            println!("Connected event received; features match expected");
                            break;
                        }
                        _ => {
                            // ignore other events
                        }
                    }
                }
            }
        })
        .await
        .is_err()
        {
            // If the test times out, kill the app process and fail the test
            edge.kill().expect("Failed to kill the app process");
            edge.wait().expect("Failed to wait for the app process");
            panic!("Test timed out waiting for connected event");
        }

        unleash_features_cache.insert(
            cache_key(&upstream_known_token),
            features_from_disk("../examples/hostedexample.json"),
        );

        if tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if let Some(Ok(event)) = stream.next().await {
                    match event {
                        eventsource_client::SSE::Event(event)
                            if event.event_type == "unleash-updated" =>
                        {
                            let update =
                                serde_json::from_str::<ClientFeatures>(&event.data).unwrap();
                            assert_eq!(initial_features.query, update.query);
                            assert_eq!(initial_features.version, update.version);
                            assert_ne!(initial_features.features, update.features);
                            println!("Updated event received; features match expected");
                            break;
                        }
                        _ => {
                            // ignore other events
                        }
                    }
                }
            }
        })
        .await
        .is_err()
        {
            // If the test times out, kill the app process and fail the test
            edge.kill().expect("Failed to kill the app process");
            edge.wait().expect("Failed to wait for the app process");
            panic!("Test timed out waiting for update event");
        }

        edge.kill().expect("Failed to kill the app process");
        edge.wait().expect("Failed to wait for the app process");
    }

    use actix_http::HttpService;
    use actix_http_test::{test_server, TestServer};
    use actix_service::map_config;
    use actix_web::dev::AppConfig;
    use actix_web::{web, App};
    use unleash_types::client_metrics::ConnectVia;
    use unleash_yggdrasil::EngineState;

    use unleash_edge::auth::token_validator::TokenValidator;
    use unleash_edge::metrics::client_metrics::MetricsCache;

    async fn upstream_server(
        upstream_token_cache: Arc<DashMap<String, EdgeToken>>,
        upstream_features_cache: Arc<FeatureCache>,
        upstream_engine_cache: Arc<DashMap<String, EngineState>>,
        upstream_broadcaster: Arc<Broadcaster>,
    ) -> TestServer {
        let token_validator = Arc::new(TokenValidator {
            unleash_client: Arc::new(Default::default()),
            token_cache: upstream_token_cache.clone(),
            persistence: None,
        });

        test_server(move || {
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
                    .app_data(web::Data::from(upstream_broadcaster.clone()))
                    .app_data(web::Data::from(upstream_engine_cache.clone()))
                    .app_data(web::Data::from(upstream_token_cache.clone()))
                    .app_data(web::Data::new(metrics_cache))
                    .app_data(web::Data::new(connect_via))
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
