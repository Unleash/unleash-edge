pub mod auth;
#[cfg(not(tarpaulin_include))]
pub mod builder;
pub mod client_api;
pub mod delta_cache;
pub mod delta_cache_manager;
pub mod delta_filters;
pub mod edge_api;
pub mod feature_cache;
pub mod filters;
pub mod frontend_api;
pub mod health_checker;
pub mod http;
pub mod internal_backstage;
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
pub mod urls;

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::BufReader;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::auth::token_validator::TokenValidator;
    use crate::delta_cache_manager::DeltaCacheManager;
    use crate::feature_cache::FeatureCache;
    use actix_http::HttpService;
    use actix_http_test::{TestServer, test_server};
    use actix_service::map_config;
    use actix_web::dev::AppConfig;
    use actix_web::{App, web};
    use dashmap::DashMap;
    use unleash_edge_client_metrics::MetricsCache;
    use unleash_edge_types::EdgeToken;
    use unleash_types::client_features::ClientFeatures;
    use unleash_types::client_metrics::ConnectVia;
    use unleash_yggdrasil::EngineState;

    pub fn features_from_disk(path: &str) -> ClientFeatures {
        let path = PathBuf::from(path);
        let file = fs::File::open(path).unwrap();
        let reader = BufReader::new(file);
        serde_json::from_reader(reader).unwrap()
    }

    pub async fn upstream_server(
        upstream_token_cache: Arc<DashMap<String, EdgeToken>>,
        upstream_features_cache: Arc<FeatureCache>,
        upstream_delta_cache_manager: Arc<DeltaCacheManager>,
        upstream_engine_cache: Arc<DashMap<String, EngineState>>,
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
                    .app_data(web::Data::from(upstream_delta_cache_manager.clone()))
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
