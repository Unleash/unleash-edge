use crate::auth::token_validator::TokenValidator;
use crate::types::{EdgeToken, TokenType, TokenValidationStatus};
use actix_web::{
    body::MessageBody,
    dev::{ServiceRequest, ServiceResponse},
    web::Data,
    HttpResponse,
};
use tokio::sync::RwLock;
use tracing::instrument;

#[instrument(skip(srv, req, validator))]
pub async fn validate_token(
    token: EdgeToken,
    validator: Data<RwLock<TokenValidator>>,
    req: ServiceRequest,
    srv: crate::middleware::as_async_middleware::Next<impl MessageBody + 'static>,
) -> Result<ServiceResponse<impl MessageBody>, actix_web::Error> {
    let mut validator_lock = validator.write().await;

    let known_token = validator_lock.register_token(token.token.clone()).await?;
    let res = match known_token.status {
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
    };
    Ok(res)
}
