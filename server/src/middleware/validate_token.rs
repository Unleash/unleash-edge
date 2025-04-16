use crate::auth::token_validator::TokenValidator;
use crate::types::{EdgeToken, TokenType, TokenValidationStatus};
use actix_web::{
    HttpResponse,
    body::MessageBody,
    dev::{ServiceRequest, ServiceResponse},
    web::Data,
};
use dashmap::DashMap;

pub async fn validate_token(
    token: EdgeToken,
    req: ServiceRequest,
    srv: crate::middleware::as_async_middleware::Next<impl MessageBody + 'static>,
) -> Result<ServiceResponse<impl MessageBody>, actix_web::Error> {
    let maybe_validator = req.app_data::<Data<TokenValidator>>();
    let token_cache = req
        .app_data::<Data<DashMap<String, EdgeToken>>>()
        .unwrap()
        .clone()
        .into_inner();

    if token.status == TokenValidationStatus::Trusted {
        return Ok(srv.call(req).await?.map_into_left_body());
    }

    match maybe_validator {
        Some(validator) => {
            let known_token = validator.register_token(token.token.clone()).await?;
            let res = match known_token.status {
                TokenValidationStatus::Validated => match known_token.token_type {
                    Some(TokenType::Frontend) => {
                        if req.path().contains("/api/frontend") || req.path().contains("/api/proxy")
                        {
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
                TokenValidationStatus::Trusted => unreachable!(),
            };
            Ok(res)
        }
        None => {
            let res = match token_cache.get(&token.token) {
                Some(t) => {
                    let token = t.value();
                    match token.token_type {
                        Some(TokenType::Client) => {
                            if req.path().contains("/api/client") {
                                srv.call(req).await?.map_into_left_body()
                            } else {
                                req.into_response(HttpResponse::Forbidden().finish())
                                    .map_into_right_body()
                            }
                        }
                        Some(TokenType::Frontend) => {
                            if req.path().contains("/api/frontend")
                                || req.path().contains("/api/proxy")
                            {
                                srv.call(req).await?.map_into_left_body()
                            } else {
                                req.into_response(HttpResponse::Forbidden().finish())
                                    .map_into_right_body()
                            }
                        }
                        None => srv.call(req).await?.map_into_left_body(),
                        _ => req
                            .into_response(HttpResponse::Forbidden().finish())
                            .map_into_right_body(),
                    }
                }
                None => req
                    .into_response(HttpResponse::Forbidden().finish())
                    .map_into_right_body(),
            };

            Ok(res)
        }
    }
}
