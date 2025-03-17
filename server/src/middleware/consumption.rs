use crate::http::headers::UNLEASH_INTERVAL;
use crate::metrics::edge_metrics::EdgeInstanceData;
use actix_http::body::MessageBody;
use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::web::Data;
use tracing::debug;

pub async fn connection_consumption(
    req: ServiceRequest,
    srv: crate::middleware::as_async_middleware::Next<impl MessageBody + 'static>,
) -> Result<ServiceResponse<impl MessageBody>, actix_web::Error> {
    if req.path().starts_with("/api/client/features")
        || req.path().starts_with("/api/client/delta")
        || req.path().starts_with("/api/client/metrics")
    {
        if let Some(instance_data) = req.app_data::<Data<EdgeInstanceData>>() {
            let data = instance_data.get_ref().clone();
            let interval = req
                .headers()
                .get(UNLEASH_INTERVAL)
                .and_then(|h| h.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok());

            data.observe_connection_consumption(req.path(), interval);
        }
    }
    srv.call(req).await
}

pub async fn request_consumption(
    req: ServiceRequest,
    srv: crate::middleware::as_async_middleware::Next<impl MessageBody + 'static>,
) -> Result<ServiceResponse<impl MessageBody>, actix_web::Error> {
    if req.path().starts_with("/api/frontend") {
        if let Some(instance_data) = req.app_data::<Data<EdgeInstanceData>>() {
            let data = instance_data.get_ref().clone();
            data.observe_request_consumption();
            debug!("Observed frontend request for path: {}", req.path());
        }
    }
    srv.call(req).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::edge_metrics::EdgeInstanceData;
    use crate::middleware::as_async_middleware::as_async_middleware;
    use actix_web::{App, HttpResponse, test};

    #[test]
    async fn test_backend_consumption() {
        let instance_data = EdgeInstanceData::new("test");
        let app = test::init_service(
            App::new()
                .app_data(Data::new(instance_data.clone()))
                .wrap(as_async_middleware(connection_consumption))
                .route(
                    "/api/client/features",
                    actix_web::web::get().to(|| async { HttpResponse::Ok() }),
                ),
        )
        .await;

        let req = test::TestRequest::get()
            .uri("/api/client/features")
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success());
    }

    #[test]
    async fn test_frontend_consumption() {
        let instance_data = EdgeInstanceData::new("test");
        let app = test::init_service(
            App::new()
                .app_data(Data::new(instance_data.clone()))
                .wrap(as_async_middleware(request_consumption))
                .route(
                    "/api/frontend/features",
                    actix_web::web::get().to(|| async { HttpResponse::Ok() }),
                ),
        )
        .await;

        let req = test::TestRequest::get()
            .uri("/api/frontend/features")
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success());
    }
}
