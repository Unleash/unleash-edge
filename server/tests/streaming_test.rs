use dashmap::DashMap;
use eventsource_client::Client;
use futures::{future, StreamExt, TryStreamExt};
use reqwest::Url;
use std::{
    fs,
    io::BufReader,
    path::PathBuf,
    str::FromStr,
    sync::{Arc, Mutex},
};
use unleash_edge::{
    http::{
        broadcaster::Broadcaster, feature_refresher::FeatureRefresher,
        unleash_client::UnleashClient,
    },
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
    let unleash_features_cache: Arc<DashMap<String, ClientFeatures>> = Arc::new(DashMap::default());
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

    println!("Upstream server started at: {}", unleash_server.url("/"));
    let edge = edge_server(&unleash_server.url("/"), upstream_known_token.clone()).await;

    let es_client = eventsource_client::ClientBuilder::for_url(&edge.url("/api/client/streaming"))
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
    while let Some(Ok(event)) = stream.next().await {
        match event {
            eventsource_client::SSE::Event(event) if event.event_type == "unleash-connected" => {
                println!("🚀Connected to edge server\n\n");
                assert_eq!(
                    serde_json::from_str::<ClientFeatures>(&event.data).unwrap(),
                    initial_features
                );
                break;
            }
            _ => {
                // ignore other events
            }
        }
    }

    // // Update features and broadcast
    println!("🦴Updating features!");
    unleash_features_cache.insert(
        cache_key(&upstream_known_token),
        features_from_disk("../examples/hostedexample.json"),
    );
    unleash_broadcaster.broadcast().await;

    // Wait for the "updated" event
    while let Some(Ok(event)) = stream.next().await {
        match event {
            eventsource_client::SSE::Event(event) if event.event_type == "unleash-updated" => {
                println!("👨‍🚀Received features update");
                // events.lock().unwrap().push(event);
                let update = serde_json::from_str::<ClientFeatures>(&event.data).unwrap();
                assert_eq!(initial_features.query, update.query);
                assert_eq!(initial_features.version, update.version);
                assert!(initial_features.features != update.features);
                break;
            }
            _ => {
                // ignore other events
            }
        }
    }
}

use actix_http::HttpService;
use actix_http_test::{test_server, TestServer};
use actix_service::map_config;
use actix_web::dev::AppConfig;
use actix_web::{web, App};
use chrono::Duration;
use unleash_types::client_metrics::ConnectVia;
use unleash_yggdrasil::EngineState;

use unleash_edge::auth::token_validator::TokenValidator;
use unleash_edge::http::unleash_client::new_reqwest_client;
use unleash_edge::metrics::client_metrics::MetricsCache;

async fn edge_server(upstream_url: &str, token: EdgeToken) -> TestServer {
    let unleash_client = Arc::new(UnleashClient::from_url(
        Url::parse(upstream_url).unwrap(),
        "Authorization".into(),
        new_reqwest_client(
            "something".into(),
            false,
            None,
            None,
            Duration::seconds(5),
            Duration::seconds(5),
            "test-client".into(),
        )
        .unwrap(),
    ));

    let features_cache: Arc<DashMap<String, ClientFeatures>> = Arc::new(DashMap::default());
    let token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());
    token_cache.insert(token.token.clone(), token.clone());
    let engine_cache: Arc<DashMap<String, EngineState>> = Arc::new(DashMap::default());
    let broadcaster = Broadcaster::new(features_cache.clone());
    let feature_refresher = Arc::new(FeatureRefresher {
        unleash_client: unleash_client.clone(),
        features_cache: features_cache.clone(),
        engine_cache: engine_cache.clone(),
        refresh_interval: Duration::seconds(6000),
        broadcaster: broadcaster.clone(),
        ..Default::default()
    });
    let token_validator = Arc::new(TokenValidator {
        unleash_client: unleash_client.clone(),
        token_cache: token_cache.clone(),
        persistence: None,
    });
    feature_refresher
        .register_token_for_refresh(token.clone(), None)
        .await;
    let refresher_for_background = feature_refresher.clone();

    let handle = tokio::spawn(async move {
        let _ = refresher_for_background
            .start_streaming_features_background_task()
            .await;
    });

    handle.await.unwrap();
    test_server(move || {
        let config =
            serde_qs::actix::QsQueryConfig::default().qs_config(serde_qs::Config::new(5, false));
        let metrics_cache = MetricsCache::default();
        let connect_via = ConnectVia {
            app_name: "edge".into(),
            instance_id: "testinstance".into(),
        };
        HttpService::new(map_config(
            App::new()
                .app_data(config)
                .app_data(web::Data::from(token_validator.clone()))
                .app_data(web::Data::from(features_cache.clone()))
                .app_data(web::Data::from(broadcaster.clone()))
                .app_data(web::Data::from(engine_cache.clone()))
                .app_data(web::Data::from(token_cache.clone()))
                .app_data(web::Data::new(metrics_cache))
                .app_data(web::Data::new(connect_via))
                .app_data(web::Data::from(feature_refresher.clone()))
                .service(
                    web::scope("/api")
                        .configure(unleash_edge::client_api::configure_client_api)
                        .configure(|cfg| {
                            unleash_edge::frontend_api::configure_frontend_api(cfg, false)
                        }),
                )
                .service(web::scope("/edge").configure(unleash_edge::edge_api::configure_edge_api)),
            |_| AppConfig::default(),
        ))
        .tcp()
    })
    .await
}
async fn upstream_server(
    upstream_token_cache: Arc<DashMap<String, EdgeToken>>,
    upstream_features_cache: Arc<DashMap<String, ClientFeatures>>,
    upstream_engine_cache: Arc<DashMap<String, EngineState>>,
    upstream_broadcaster: Arc<Broadcaster>,
) -> TestServer {
    let token_validator = Arc::new(TokenValidator {
        unleash_client: Arc::new(Default::default()),
        token_cache: upstream_token_cache.clone(),
        persistence: None,
    });

    test_server(move || {
        let config =
            serde_qs::actix::QsQueryConfig::default().qs_config(serde_qs::Config::new(5, false));
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
                .service(web::scope("/edge").configure(unleash_edge::edge_api::configure_edge_api)),
            |_| AppConfig::default(),
        ))
        .tcp()
    })
    .await
}
