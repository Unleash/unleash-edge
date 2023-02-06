use actix_web::{
    body::MessageBody,
    dev::{ServiceRequest, ServiceResponse},
    web::Data,
    HttpResponse,
};

use crate::types::{EdgeSource, EdgeToken, TokenType, TokenValidationStatus};
use tokio::sync::{mpsc::Sender, RwLock};

pub async fn validate_token(
    token: EdgeToken,
    provider: Data<RwLock<dyn EdgeSource>>,
    sender: Data<Sender<EdgeToken>>,
    req: ServiceRequest,
    srv: crate::middleware::as_async_middleware::Next<impl MessageBody + 'static>,
) -> Result<ServiceResponse<impl MessageBody>, actix_web::Error> {
    let res = match provider
        .read()
        .await
        .get_token_validation_status(token.token.as_str(), sender.into_inner())
        .await
    {
        Ok(TokenValidationStatus::Validated) => {
            if req.path().contains("/api/frontend") || req.path().contains("/api/proxy") {
                if token.token_type == Some(TokenType::Frontend) {
                    srv.call(req).await?.map_into_left_body()
                } else {
                    req.into_response(HttpResponse::Forbidden().finish())
                        .map_into_right_body()
                }
            } else if req.path().contains("/api/client") {
                if token.token_type == Some(TokenType::Client) {
                    srv.call(req).await?.map_into_left_body()
                } else {
                    req.into_response(HttpResponse::Forbidden().finish())
                        .map_into_right_body()
                }
            } else {
                req.into_response(HttpResponse::NotFound().finish())
                    .map_into_right_body()
            }
        }
        Ok(TokenValidationStatus::Unknown) => req
            .into_response(HttpResponse::Unauthorized().finish())
            .map_into_right_body(),
        Ok(TokenValidationStatus::Invalid) => req
            .into_response(HttpResponse::Forbidden().finish())
            .map_into_right_body(),
        Err(_e) => req
            .into_response(HttpResponse::Unauthorized().finish())
            .map_into_right_body(),
    };
    Ok(res)
}
