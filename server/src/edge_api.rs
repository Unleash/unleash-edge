use actix_web::{
    post,
    web::{self, Data, Json},
    HttpRequest, HttpResponse,
};
use tokio::sync::RwLock;

use crate::{
    auth::token_validator::TokenValidator,
    types::{EdgeJsonResult, EdgeSource, TokenStrings, TokenValidationStatus, ValidatedTokens},
};
use crate::{
    metrics::client_metrics::MetricsCache,
    types::{BatchMetricsRequestBody, EdgeResult},
};
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
    token_provider: web::Data<RwLock<dyn EdgeSource>>,
    req: HttpRequest,
    tokens: Json<TokenStrings>,
) -> EdgeJsonResult<ValidatedTokens> {
    let maybe_validator = req.app_data::<Data<RwLock<TokenValidator>>>();
    match maybe_validator {
        Some(validator) => {
            let known_tokens = validator
                .write()
                .await
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
            tokens: token_provider
                .read()
                .await
                .filter_valid_tokens(tokens.into_inner().tokens)
                .await?,
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
    metrics_cache: web::Data<RwLock<MetricsCache>>,
) -> EdgeResult<HttpResponse> {
    {
        let mut metrics_lock = metrics_cache.write().await;

        metrics_lock.sink_metrics(&batch_metrics_request.metrics);
    }
    Ok(HttpResponse::Accepted().finish())
}

pub fn configure_edge_api(cfg: &mut web::ServiceConfig) {
    cfg.service(validate).service(metrics);
}
