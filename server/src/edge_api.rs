use crate::types::{
    EdgeJsonResult, EdgeToken, TokenStrings, TokenValidationStatus, ValidatedTokens,
};
use crate::{
    auth::token_validator::TokenValidator,
    metrics::client_metrics::MetricsCache,
    types::{BatchMetricsRequestBody, EdgeResult},
};
use actix_web::{
    post,
    web::{self, Data, Json},
    HttpRequest, HttpResponse,
};
use dashmap::DashMap;
use utoipa;

#[utoipa::path(
    path = "/edge/validate",
    responses(
        (status = 200, description = "Return valid tokens from list of tokens passed in to validate", body = ValidatedTokens)
    ),
    request_body = TokenStrings
)]
#[post("/validate")]
pub async fn validate(
    token_cache: web::Data<DashMap<String, EdgeToken>>,
    req: HttpRequest,
    tokens: Json<TokenStrings>,
) -> EdgeJsonResult<ValidatedTokens> {
    let maybe_validator = req.app_data::<Data<TokenValidator>>();
    match maybe_validator {
        Some(validator) => {
            let known_tokens = validator
                .register_tokens(tokens.into_inner().tokens)
                .await?;
            Ok(Json(ValidatedTokens {
                tokens: known_tokens
                    .into_iter()
                    .filter(|t| t.status == TokenValidationStatus::Validated)
                    .collect(),
            }))
        }
        None => Ok(Json(ValidatedTokens {
            tokens: token_cache
                .iter()
                .filter(|t| t.token_type.is_some())
                .map(|e| e.value().clone())
                .collect(),
        })),
    }
}

#[utoipa::path(
    path = "/edge/metrics",
    responses(
        (status = 202, description = "Accepted the posted metrics")
    ),
    request_body = BatchMetricsRequestBody,
    security(
        ("Authorization" = [])
    )
)]
#[post("/metrics")]
pub async fn metrics(
    batch_metrics_request: web::Json<BatchMetricsRequestBody>,
    metrics_cache: web::Data<MetricsCache>,
) -> EdgeResult<HttpResponse> {
    metrics_cache.sink_metrics(&batch_metrics_request.metrics);
    Ok(HttpResponse::Accepted().finish())
}

pub fn configure_edge_api(cfg: &mut web::ServiceConfig) {
    cfg.service(validate).service(metrics);
}
