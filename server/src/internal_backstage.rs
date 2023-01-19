use actix_web::{
    get,
    web::{self, Json},
    HttpRequest,
};
use serde::Serialize;

use crate::types::EdgeJsonResult;

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
pub async fn health(_req: HttpRequest) -> EdgeJsonResult<EdgeStatus> {
    Ok(Json(EdgeStatus::ok()))
}

pub fn configure_internal_backstage(cfg: &mut web::ServiceConfig) {
    cfg.service(health);
}

#[cfg(test)]
mod tests {
    use actix_web::{http::header::ContentType, test, web, App};

    #[actix_web::test]
    async fn test_health_ok() {
        let app = test::init_service(App::new().service(
            web::scope("/internal-backstage").configure(super::configure_internal_backstage),
        ))
        .await;
        let req = test::TestRequest::get()
            .uri("/internal-backstage/health")
            .insert_header(ContentType::json())
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success())
    }
}
