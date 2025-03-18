use crate::http::headers::UNLEASH_INTERVAL;
use crate::metrics::edge_metrics::EdgeInstanceData;
use actix_http::body::MessageBody;
use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::web::Data;

pub async fn connection_consumption(
    req: ServiceRequest,
    srv: crate::middleware::as_async_middleware::Next<impl MessageBody + 'static>,
) -> Result<ServiceResponse<impl MessageBody>, actix_web::Error> {
    let path = req.path().to_string();
    let should_observe = path.starts_with("/api/client/features")
        || path.starts_with("/api/client/delta")
        || path.starts_with("/api/client/metrics");

    let interval = if should_observe {
        req.headers()
            .get(UNLEASH_INTERVAL)
            .and_then(|h| h.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
    } else {
        None
    };

    let resp = srv.call(req).await?;
    let status_code = resp.status().as_u16();

    if should_observe && ((200..300).contains(&status_code) || status_code == 304) {
        if let Some(instance_data) = resp.request().app_data::<Data<EdgeInstanceData>>() {
            instance_data
                .get_ref()
                .observe_connection_consumption(&path, interval);
        }
    }

    Ok(resp)
}

pub async fn request_consumption(
    req: ServiceRequest,
    srv: crate::middleware::as_async_middleware::Next<impl MessageBody + 'static>,
) -> Result<ServiceResponse<impl MessageBody>, actix_web::Error> {
    let path = req.path().to_string();
    let should_observe = path.starts_with("/api/frontend");

    let resp = srv.call(req).await?;
    let status_code = resp.status().as_u16();

    if should_observe && ((200..300).contains(&status_code) || status_code == 304) {
        if let Some(instance_data) = resp.request().app_data::<Data<EdgeInstanceData>>() {
            instance_data.get_ref().observe_request_consumption();
        }
    }

    Ok(resp)
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
