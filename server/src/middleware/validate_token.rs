use crate::types::{EdgeSource, EdgeToken, TokenType, TokenValidationStatus};
use actix_web::{
    body::MessageBody,
    dev::{ServiceRequest, ServiceResponse},
    web::Data,
    HttpResponse,
};
use tokio::sync::RwLock;
use tracing::instrument;

#[instrument(skip(srv, req, provider))]
pub async fn validate_token(
    token: EdgeToken,
    provider: Data<RwLock<dyn EdgeSource>>,
    req: ServiceRequest,
    srv: crate::middleware::as_async_middleware::Next<impl MessageBody + 'static>,
) -> Result<ServiceResponse<impl MessageBody>, actix_web::Error> {
    let res = if let Some(known_token) = provider
        .read()
        .await
        .token_details(token.token.clone())
        .await?
    {
        match known_token.status {
            TokenValidationStatus::Validated => match known_token.token_type {
                Some(TokenType::Frontend) => {
                    if req.path().contains("/api/frontend") || req.path().contains("/api/proxy") {
                        srv.call(req).await?.map_into_left_body()
                    } else {
                        req.into_response(HttpResponse::Forbidden().finish())
                            .map_into_right_body()
                    }
                }
                Some(TokenType::Client) => {
                    if req.path().contains("/api/client") {
                        srv.call(req).await?.map_into_left_body()
                    } else {
                        req.into_response(HttpResponse::Forbidden().finish())
                            .map_into_right_body()
                    }
                }
                _ => req
                    .into_response(HttpResponse::Forbidden().finish())
                    .map_into_right_body(),
            },
            TokenValidationStatus::Unknown => req
                .into_response(HttpResponse::Unauthorized().finish())
                .map_into_right_body(),
            TokenValidationStatus::Invalid => req
                .into_response(HttpResponse::Forbidden().finish())
                .map_into_right_body(),
        }
    } else {
        let lock = provider.read().await;
        let _ = lock.get_token_validation_status(token.token.as_str()).await;
        drop(lock);
        req.into_response(HttpResponse::Unauthorized())
            .map_into_right_body()
    };
    Ok(res)
}
