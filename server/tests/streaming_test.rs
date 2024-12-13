use dashmap::DashMap;
use eventsource_client::Client;
use futures::{StreamExt, TryStreamExt};
use reqwest::Url;
use std::{
    str::FromStr,
    sync::{Arc, Mutex},
};
use tokio::sync::mpsc;
use tracing::event;
use unleash_edge::{
    http::{
        broadcaster::Broadcaster, feature_refresher::FeatureRefresher,
        unleash_client::UnleashClient,
    },
    // tests::{edge_server, upstream_server},
    types::{EdgeToken, TokenType, TokenValidationStatus},
};
use unleash_types::client_features::ClientFeatures;

#[actix_web::test]
async fn test_streaming() {
    let unleash_broadcaster = Broadcaster::new(Arc::new(DashMap::default()));
    let unleash_features_cache: Arc<DashMap<String, ClientFeatures>> = Arc::new(DashMap::default());
    let unleash_token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());

    let unleash_server = upstream_server(
        unleash_token_cache.clone(),
        unleash_features_cache.clone(),
        Arc::new(DashMap::default()),
        unleash_broadcaster.clone(),
    )
    .await;

    let edge = edge_server(&unleash_server.url("/")).await;

    let mut upstream_known_token = EdgeToken::from_str("dx:development.secret123").unwrap();
    upstream_known_token.status = TokenValidationStatus::Validated;
    upstream_known_token.token_type = Some(TokenType::Client);
    unleash_token_cache.insert(
        upstream_known_token.token.clone(),
        upstream_known_token.clone(),
    );

    let es_client = eventsource_client::ClientBuilder::for_url(&edge.url("/api/client/streaming"))
        .unwrap()
        .header("Authorization", &upstream_known_token.token)
        .unwrap()
        .build();
    let num_events_to_collect = 5;
    let events = Arc::new(Mutex::new(Vec::new()));
    let events_clone = events.clone();

    let handle = tokio::spawn(async move {
        let _ = es_client
            .stream()
            .take(num_events_to_collect)
            .try_for_each(|sse| {
                let events_clone = events.clone();
                async move {
                    println!("{:?}", sse);
                    events_clone.lock().unwrap().push(sse);
                    Ok(())
                }
            });
    });

    handle.await.unwrap();

    // Now we can inspect the collected events
    let collected_events = events_clone.lock().unwrap();
    print!("Collected events: {collected_events:?}");
    for (i, event) in collected_events.iter().enumerate() {
        println!("Event {}: {:?}", i, event);
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

async fn edge_server(upstream_url: &str) -> TestServer {
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
