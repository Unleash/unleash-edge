use actix_web::{
    get,
    web::{self, Json},
};
use actix_web_opentelemetry::PrometheusMetricsHandler;
use autometrics::autometrics;
use serde::Serialize;
use tokio::sync::RwLock;

use crate::types::{BuildInfo, EdgeJsonResult, EdgeSource, EdgeToken};

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
#[autometrics]
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
    edge_source: web::Data<RwLock<dyn EdgeSource>>,
) -> EdgeJsonResult<Vec<EdgeToken>> {
    let all_tokens = edge_source.read().await.get_known_tokens().await?;
    Ok(Json(all_tokens))
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
    use actix_web::{body::MessageBody, http::header::ContentType, test, web, App};

    use crate::types::BuildInfo;

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
