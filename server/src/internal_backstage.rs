use std::collections::HashMap;

use actix_web::{
    get,
    web::{self, Json},
};
use dashmap::DashMap;
use iter_tools::Itertools;
use serde::{Deserialize, Serialize};
use unleash_types::client_features::ClientFeatures;
use unleash_types::client_metrics::ClientApplication;

use crate::http::feature_refresher::FeatureRefresher;
use crate::metrics::actix_web_metrics::PrometheusMetricsHandler;
use crate::metrics::client_metrics::MetricsCache;
use crate::types::{BuildInfo, EdgeJsonResult, EdgeToken, TokenInfo, TokenRefresh};
use crate::types::{ClientMetric, MetricsInfo, Status};
use crate::{auth::token_validator::TokenValidator, cli::InternalBackstageArgs};
use crate::{error::EdgeError, feature_cache::FeatureCache};

#[derive(Debug, Serialize, Deserialize)]
pub struct EdgeStatus {
    pub status: Status,
}

impl EdgeStatus {
    pub fn ok() -> Self {
        EdgeStatus { status: Status::Ok }
    }
    pub fn not_ready() -> Self {
        EdgeStatus {
            status: Status::NotReady,
        }
    }

    pub fn ready() -> Self {
        EdgeStatus {
            status: Status::Ready,
        }
    }
}

#[get("/health")]
pub async fn health() -> EdgeJsonResult<EdgeStatus> {
    Ok(Json(EdgeStatus::ok()))
}

#[get("/info")]
pub async fn info() -> EdgeJsonResult<BuildInfo> {
    let data = BuildInfo::default();
    Ok(Json(data))
}

#[get("/ready")]
pub async fn ready(
    token_cache: web::Data<DashMap<String, EdgeToken>>,
    features_cache: web::Data<FeatureCache>,
) -> EdgeJsonResult<EdgeStatus> {
    if !token_cache.is_empty() && features_cache.is_empty() {
        Err(EdgeError::NotReady)
    } else {
        Ok(Json(EdgeStatus::ready()))
    }
}

#[get("/tokens")]
pub async fn tokens(
    token_cache: web::Data<DashMap<String, EdgeToken>>,
    feature_refresher: Option<web::Data<FeatureRefresher>>,
    token_validator: Option<web::Data<TokenValidator>>,
) -> EdgeJsonResult<TokenInfo> {
    match (feature_refresher, token_validator) {
        (Some(feature_refresher), Some(token_validator)) => {
            Ok(Json(get_token_info(feature_refresher, token_validator)))
        }
        _ => Ok(Json(get_offline_token_info(token_cache))),
    }
}

fn get_token_info(
    feature_refresher: web::Data<FeatureRefresher>,
    token_validator: web::Data<TokenValidator>,
) -> TokenInfo {
    let refreshes: Vec<TokenRefresh> = feature_refresher
        .tokens_to_refresh
        .iter()
        .map(|e| e.value().clone())
        .map(|f| TokenRefresh {
            token: crate::tokens::anonymize_token(&f.token),
            ..f
        })
        .collect();
    let token_validation_status: Vec<EdgeToken> = token_validator
        .token_cache
        .iter()
        .map(|e| e.value().clone())
        .map(|t| crate::tokens::anonymize_token(&t))
        .collect();
    TokenInfo {
        token_refreshes: refreshes,
        token_validation_status,
    }
}

fn get_offline_token_info(token_cache: web::Data<DashMap<String, EdgeToken>>) -> TokenInfo {
    let edge_tokens: Vec<EdgeToken> = token_cache
        .iter()
        .map(|e| e.value().clone())
        .map(|t| crate::tokens::anonymize_token(&t))
        .collect();
    TokenInfo {
        token_refreshes: vec![],
        token_validation_status: edge_tokens,
    }
}

#[get("/metricsbatch")]
pub async fn metrics_batch(metrics_cache: web::Data<MetricsCache>) -> EdgeJsonResult<MetricsInfo> {
    let applications: Vec<ClientApplication> = metrics_cache
        .applications
        .iter()
        .map(|e| e.value().clone())
        .collect_vec();
    let metrics = metrics_cache
        .metrics
        .iter()
        .map(|e| ClientMetric {
            key: e.key().clone(),
            bucket: e.value().clone(),
        })
        .collect_vec();
    Ok(Json(MetricsInfo {
        applications,
        metrics,
    }))
}

#[get("/features")]
pub async fn features(
    features_cache: web::Data<FeatureCache>,
) -> EdgeJsonResult<HashMap<String, ClientFeatures>> {
    let features = features_cache
        .iter()
        .map(|e| (e.key().clone(), e.value().clone()))
        .collect();
    Ok(Json(features))
}

pub fn configure_internal_backstage(
    cfg: &mut web::ServiceConfig,
    metrics_handler: PrometheusMetricsHandler,
    internal_backtage_args: InternalBackstageArgs,
) {
    cfg.service(health).service(info).service(ready);
    if !internal_backtage_args.disable_tokens_endpoint {
        cfg.service(tokens);
    }
    if !internal_backtage_args.disable_metrics_endpoint {
        cfg.service(web::resource("/metrics").route(web::get().to(metrics_handler)));
    }
    if !internal_backtage_args.disable_metrics_batch_endpoint {
        cfg.service(metrics_batch);
    }
    if !internal_backtage_args.disable_features_endpoint {
        cfg.service(features);
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;
    use std::sync::Arc;

    use actix_web::body::MessageBody;
    use actix_web::http::header::ContentType;
    use actix_web::test;
    use actix_web::{web, App};
    use chrono::Duration;
    use dashmap::DashMap;
    use unleash_types::client_features::{ClientFeature, ClientFeatures};
    use unleash_yggdrasil::EngineState;

    use crate::auth::token_validator::TokenValidator;
    use crate::feature_cache::FeatureCache;
    use crate::http::feature_refresher::FeatureRefresher;
    use crate::http::unleash_client::UnleashClient;
    use crate::internal_backstage::EdgeStatus;
    use crate::middleware;
    use crate::tests::upstream_server;
    use crate::tokens::cache_key;
    use crate::types::{BuildInfo, EdgeToken, Status, TokenInfo, TokenType, TokenValidationStatus};

    #[actix_web::test]
    async fn test_health_ok() {
        let app = test::init_service(
            App::new().service(web::scope("/internal-backstage").service(super::health)),
        )
        .await;
        let req = test::TestRequest::get()
            .uri("/internal-backstage/health")
            .insert_header(ContentType::json())
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success())
    }

    #[actix_web::test]
    async fn test_build_info_ok() {
        let app = test::init_service(
            App::new().service(web::scope("/internal-backstage").service(super::info)),
        )
        .await;
        let req = test::TestRequest::get()
            .uri("/internal-backstage/info")
            .insert_header(ContentType::json())
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success());
        let body = resp.into_body().try_into_bytes().unwrap();
        let info: BuildInfo = serde_json::from_slice(&body).unwrap();
        assert_eq!(info.app_name, "unleash-edge");
    }

    #[actix_web::test]
    async fn test_ready_endpoint_with_tokens_without_toggles() {
        let client_features = FeatureCache::default();
        let client_features_arc = Arc::new(client_features);
        let token_cache: DashMap<String, EdgeToken> = DashMap::default();
        let token = EdgeToken::from_str("[]:fancyenvironment.somerandomsecretstring").unwrap();
        token_cache.insert(token.token.clone(), token);
        let token_cache_arc = Arc::new(token_cache);
        let app = test::init_service(
            App::new()
                .app_data(web::Data::from(client_features_arc))
                .app_data(web::Data::from(token_cache_arc))
                .service(web::scope("/internal-backstage").service(super::ready)),
        )
        .await;
        let req = test::TestRequest::get()
            .uri("/internal-backstage/ready")
            .insert_header(ContentType::json())
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_server_error());
        let status: EdgeStatus = test::read_body_json(resp).await;
        assert_eq!(status.status, Status::NotReady);
    }

    #[actix_web::test]
    async fn test_ready_endpoint_with_tokens_and_toggles() {
        let features = ClientFeatures {
            features: vec![ClientFeature {
                name: "test".to_string(),
                ..ClientFeature::default()
            }],
            query: None,
            segments: None,
            version: 2,
        };
        let client_features = FeatureCache::default();
        client_features.insert(
            "testproject:testenvironment.testtoken".into(),
            features.clone(),
        );
        let client_features_arc = Arc::new(client_features);
        let token_cache: DashMap<String, EdgeToken> = DashMap::default();
        let token = EdgeToken::from_str("[]:fancyenvironment.somerandomsecretstring").unwrap();
        token_cache.insert(token.token.clone(), token);
        let token_cache_arc = Arc::new(token_cache);
        let app = test::init_service(
            App::new()
                .app_data(web::Data::from(client_features_arc))
                .app_data(web::Data::from(token_cache_arc))
                .service(web::scope("/internal-backstage").service(super::ready)),
        )
        .await;
        let req = test::TestRequest::get()
            .uri("/internal-backstage/ready")
            .insert_header(ContentType::json())
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success());
        let status: EdgeStatus = test::read_body_json(resp).await;
        assert_eq!(status.status, Status::Ready);
    }

    #[actix_web::test]
    async fn test_ready_endpoint_without_tokens_and_toggles() {
        let client_features = FeatureCache::default();
        let client_features_arc = Arc::new(client_features);
        let token_cache: DashMap<String, EdgeToken> = DashMap::default();
        let token_cache_arc = Arc::new(token_cache);
        let app = test::init_service(
            App::new()
                .app_data(web::Data::from(client_features_arc))
                .app_data(web::Data::from(token_cache_arc))
                .service(web::scope("/internal-backstage").service(super::ready)),
        )
        .await;
        let req = test::TestRequest::get()
            .uri("/internal-backstage/ready")
            .insert_header(ContentType::json())
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success());
        let status: EdgeStatus = test::read_body_json(resp).await;
        assert_eq!(status.status, Status::Ready);
    }

    #[actix_web::test]
    async fn if_no_tokens_has_been_received_returns_empty_lists() {
        let upstream_server = upstream_server(
            Arc::new(DashMap::default()),
            Arc::new(FeatureCache::default()),
            Arc::new(DashMap::default()),
        )
        .await;
        let unleash_client =
            UnleashClient::new_insecure(upstream_server.url("/").as_str()).unwrap();
        let arc_unleash_client = Arc::new(unleash_client);
        let feature_refresher = FeatureRefresher {
            tokens_to_refresh: Arc::new(DashMap::default()),
            unleash_client: arc_unleash_client.clone(),
            ..Default::default()
        };
        let token_validator = TokenValidator {
            unleash_client: arc_unleash_client.clone(),
            token_cache: Arc::new(DashMap::default()),
            persistence: None,
        };
        let token_cache: DashMap<String, EdgeToken> = DashMap::default();
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(feature_refresher))
                .app_data(web::Data::new(token_validator))
                .app_data(web::Data::new(token_cache))
                .service(web::scope("/internal-backstage").service(super::tokens)),
        )
        .await;
        let req = test::TestRequest::get()
            .uri("/internal-backstage/tokens")
            .insert_header(ContentType::json())
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success());
        let status: TokenInfo = test::read_body_json(resp).await;
        assert!(status.token_refreshes.is_empty());
        assert!(status.token_validation_status.is_empty());
    }

    #[actix_web::test]
    async fn returns_validated_tokens_when_dynamic() {
        let upstream_features_cache = Arc::new(FeatureCache::default());
        let upstream_token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());
        let upstream_engine_cache: Arc<DashMap<String, EngineState>> = Arc::new(DashMap::default());
        let server = upstream_server(
            upstream_token_cache.clone(),
            upstream_features_cache.clone(),
            upstream_engine_cache.clone(),
        )
        .await;
        let upstream_features = crate::tests::features_from_disk("../examples/hostedexample.json");
        let mut upstream_known_token = EdgeToken::from_str("dx:development.secret123").unwrap();
        upstream_known_token.status = TokenValidationStatus::Validated;
        upstream_known_token.token_type = Some(TokenType::Client);
        upstream_token_cache.insert(
            upstream_known_token.token.clone(),
            upstream_known_token.clone(),
        );
        upstream_features_cache.insert(cache_key(&upstream_known_token), upstream_features.clone());
        let unleash_client = Arc::new(UnleashClient::new(server.url("/").as_str(), None).unwrap());
        let features_cache: Arc<FeatureCache> = Arc::new(FeatureCache::default());
        let token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());
        let engine_cache: Arc<DashMap<String, EngineState>> = Arc::new(DashMap::default());
        let feature_refresher = Arc::new(FeatureRefresher {
            unleash_client: unleash_client.clone(),
            features_cache: features_cache.clone(),
            engine_cache: engine_cache.clone(),
            refresh_interval: Duration::seconds(6000),
            strict: false,
            ..Default::default()
        });
        let token_validator = Arc::new(TokenValidator {
            unleash_client: unleash_client.clone(),
            token_cache: token_cache.clone(),
            persistence: None,
        });
        let local_app = test::init_service(
            App::new()
                .app_data(web::Data::from(token_validator.clone()))
                .app_data(web::Data::from(features_cache.clone()))
                .app_data(web::Data::from(engine_cache.clone()))
                .app_data(web::Data::from(token_cache.clone()))
                .app_data(web::Data::from(feature_refresher.clone()))
                .service(web::scope("/internal-backstage").service(super::tokens))
                .service(
                    web::scope("/api")
                        .wrap(middleware::as_async_middleware::as_async_middleware(
                            middleware::validate_token::validate_token,
                        ))
                        .configure(crate::client_api::configure_client_api),
                ),
        )
        .await;
        let client_request = test::TestRequest::get()
            .uri("/api/client/features")
            .insert_header(ContentType::json())
            .insert_header(("Authorization", upstream_known_token.token.clone()))
            .to_request();
        let res = test::call_service(&local_app, client_request).await;
        assert_eq!(res.status(), actix_http::StatusCode::OK);
        let tokens_request = test::TestRequest::get()
            .uri("/internal-backstage/tokens")
            .insert_header(ContentType::json())
            .to_request();
        let token_res = test::call_service(&local_app, tokens_request).await;
        let status: TokenInfo = test::read_body_json(token_res).await;
        assert_eq!(status.token_refreshes.len(), 1);
        assert_eq!(status.token_validation_status.len(), 1);
    }

    #[actix_web::test]
    async fn returns_validated_tokens_when_strict() {
        let upstream_features_cache = Arc::new(FeatureCache::default());
        let upstream_token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());
        let upstream_engine_cache: Arc<DashMap<String, EngineState>> = Arc::new(DashMap::default());
        let server = upstream_server(
            upstream_token_cache.clone(),
            upstream_features_cache.clone(),
            upstream_engine_cache.clone(),
        )
        .await;
        let upstream_features = crate::tests::features_from_disk("../examples/hostedexample.json");
        let mut upstream_known_token = EdgeToken::from_str("dx:development.secret123").unwrap();
        upstream_known_token.status = TokenValidationStatus::Validated;
        upstream_known_token.token_type = Some(TokenType::Client);
        upstream_token_cache.insert(
            upstream_known_token.token.clone(),
            upstream_known_token.clone(),
        );
        upstream_features_cache.insert(cache_key(&upstream_known_token), upstream_features.clone());
        let unleash_client = Arc::new(UnleashClient::new(server.url("/").as_str(), None).unwrap());
        let features_cache: Arc<FeatureCache> = Arc::new(FeatureCache::default());
        let token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());
        let engine_cache: Arc<DashMap<String, EngineState>> = Arc::new(DashMap::default());
        let feature_refresher = Arc::new(FeatureRefresher {
            unleash_client: unleash_client.clone(),
            features_cache: features_cache.clone(),
            engine_cache: engine_cache.clone(),
            refresh_interval: Duration::seconds(6000),
            ..Default::default()
        });
        let token_validator = Arc::new(TokenValidator {
            unleash_client: unleash_client.clone(),
            token_cache: token_cache.clone(),
            persistence: None,
        });
        let local_app = test::init_service(
            App::new()
                .app_data(web::Data::from(token_validator.clone()))
                .app_data(web::Data::from(features_cache.clone()))
                .app_data(web::Data::from(engine_cache.clone()))
                .app_data(web::Data::from(token_cache.clone()))
                .app_data(web::Data::from(feature_refresher.clone()))
                .service(web::scope("/internal-backstage").service(super::tokens))
                .service(
                    web::scope("/api")
                        .wrap(middleware::as_async_middleware::as_async_middleware(
                            middleware::validate_token::validate_token,
                        ))
                        .configure(crate::client_api::configure_client_api),
                ),
        )
        .await;
        let client_request = test::TestRequest::get()
            .uri("/api/client/features")
            .insert_header(ContentType::json())
            .insert_header(("Authorization", upstream_known_token.token.clone()))
            .to_request();
        let res = test::call_service(&local_app, client_request).await;
        assert_eq!(res.status(), actix_http::StatusCode::FORBIDDEN);
    }
}
