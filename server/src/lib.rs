pub mod auth;
#[cfg(not(tarpaulin_include))]
pub mod builder;
#[cfg(not(tarpaulin_include))]
pub mod cli;
pub mod client_api;
pub mod edge_api;
#[cfg(not(tarpaulin_include))]
pub mod error;
pub mod filters;
pub mod frontend_api;
pub mod health_checker;
pub mod http;
pub mod internal_backstage;
pub mod metrics;
pub mod middleware;
pub mod offline;
#[cfg(not(tarpaulin_include))]
pub mod openapi;
pub mod persistence;
#[cfg(not(tarpaulin_include))]
pub mod prom_metrics;

pub mod ready_checker;
#[cfg(not(tarpaulin_include))]
pub mod tls;
pub mod tokens;
pub mod types;
pub mod urls;
#[cfg(test)]
pub mod tests {
    use std::fs;
    use std::io::BufReader;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::client_api::configure_client_api;
    use crate::middleware;
    use actix_http::HttpService;
    use actix_http_test::{test_server, TestServer};
    use actix_service::map_config;
    use actix_web::dev::{AppConfig, Url};
    use actix_web::web::Data;
    use actix_web::{test, web, App};
    use chrono::Duration;
    use dashmap::DashMap;
    use unleash_types::client_features::ClientFeatures;
    use unleash_types::client_metrics::ConnectVia;
    use unleash_yggdrasil::EngineState;

    use crate::auth::token_validator::TokenValidator;
    use crate::http::broadcaster::{self, Broadcaster};
    use crate::http::feature_refresher::FeatureRefresher;
    use crate::http::unleash_client::UnleashClient;
    use crate::metrics::client_metrics::MetricsCache;
    use crate::types::EdgeToken;

    pub fn features_from_disk(path: &str) -> ClientFeatures {
        let path = PathBuf::from(path);
        let file = fs::File::open(path).unwrap();
        let reader = BufReader::new(file);
        serde_json::from_reader(reader).unwrap()
    }

    pub async fn edge_server(upstream_url: &str) -> TestServer {
        let unleash_client = Arc::new(UnleashClient::new(upstream_url, None).unwrap());

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
                    .app_data(web::Data::from(features_cache.clone()))
                    .app_data(web::Data::from(broadcaster.clone()))
                    .app_data(web::Data::from(engine_cache.clone()))
                    .app_data(web::Data::from(token_cache.clone()))
                    .app_data(web::Data::new(metrics_cache))
                    .app_data(web::Data::new(connect_via))
                    .app_data(web::Data::from(feature_refresher.clone()))
                    .service(
                        web::scope("/api")
                            .configure(crate::client_api::configure_client_api)
                            .configure(|cfg| {
                                crate::frontend_api::configure_frontend_api(cfg, false)
                            }),
                    )
                    .service(web::scope("/edge").configure(crate::edge_api::configure_edge_api)),
                |_| AppConfig::default(),
            ))
            .tcp()
        })
        .await
    }

    pub async fn upstream_server(
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
                            .configure(crate::client_api::configure_client_api)
                            .configure(|cfg| {
                                crate::frontend_api::configure_frontend_api(cfg, false)
                            }),
                    )
                    .service(web::scope("/edge").configure(crate::edge_api::configure_edge_api)),
                |_| AppConfig::default(),
            ))
            .tcp()
        })
        .await
    }
}
