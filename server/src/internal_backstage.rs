use crate::auth::token_validator::TokenValidator;
use crate::http::feature_refresher::FeatureRefresher;
use crate::metrics::actix_web_metrics::PrometheusMetricsHandler;
use crate::types::{BuildInfo, EdgeJsonResult, EdgeToken, TokenInfo, TokenRefresh};
use actix_web::{
    get,
    web::{self, Json},
};
use serde::Serialize;
#[derive(Debug, Serialize)]
pub struct EdgeStatus {
    status: String,
}

impl EdgeStatus {
    pub fn ok() -> Self {
        EdgeStatus {
            status: "OK".into(),
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

#[get("/tokens")]
pub async fn tokens(
    feature_refresher: web::Data<FeatureRefresher>,
    token_validator: web::Data<TokenValidator>,
) -> EdgeJsonResult<TokenInfo> {
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
    Ok(Json(TokenInfo {
        token_refreshes: refreshes,
        token_validation_status,
    }))
}
pub fn configure_internal_backstage(
    cfg: &mut web::ServiceConfig,
    metrics_handler: PrometheusMetricsHandler,
) {
    cfg.service(health)
        .service(info)
        .service(tokens)
        .service(web::resource("/metrics").route(web::get().to(metrics_handler)));
}

#[cfg(test)]
mod tests {
    use crate::types::BuildInfo;
    use actix_web::body::MessageBody;
    use actix_web::http::header::ContentType;
    use actix_web::test;
    use actix_web::{web, App};

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
}
