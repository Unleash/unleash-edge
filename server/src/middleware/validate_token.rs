use crate::auth::token_validator::TokenValidator;
use crate::cli::EdgeMode;
use crate::http::feature_refresher::FeatureRefresher;
use crate::types::{EdgeToken, TokenType, TokenValidationStatus};
use actix_web::{
    body::MessageBody,
    dev::{ServiceRequest, ServiceResponse},
    web::Data,
    HttpResponse,
};
use dashmap::DashMap;

pub async fn validate_token(
    token: EdgeToken,
    req: ServiceRequest,
    srv: crate::middleware::as_async_middleware::Next<impl MessageBody + 'static>,
) -> Result<ServiceResponse<impl MessageBody>, actix_web::Error> {
    let maybe_validator = req.app_data::<Data<TokenValidator>>();
    let maybe_refresher = req.app_data::<Data<FeatureRefresher>>();
    let maybe_edge_mode = req.app_data::<Data<EdgeMode>>();
    let token_cache = req
        .app_data::<Data<DashMap<String, EdgeToken>>>()
        .unwrap()
        .clone()
        .into_inner();
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
                        if maybe_refresher.is_some() {
                            let _ = maybe_refresher
                                .unwrap()
                                .register_token_for_refresh(
                                    known_token.clone(),
                                    maybe_edge_mode
                                        .map(|e| match e.as_ref() {
                                            EdgeMode::Offline(_) => 10,
                                            EdgeMode::Edge(args) => {
                                                args.features_refresh_interval_seconds
                                            }
                                        })
                                        .unwrap_or(10),
                                )
                                .await;
                        }
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
        None => {
            let res = match token_cache.get(&token.token) {
                Some(_) => srv.call(req).await?.map_into_left_body(),
                None => req
                    .into_response(HttpResponse::Forbidden().finish())
                    .map_into_right_body(),
            };

            Ok(res)
        }
    }
}
