use actix_web::{
    post,
    web::{self, Json},
    HttpResponse,
};
use tokio::sync::RwLock;

use crate::types::{EdgeJsonResult, EdgeSource, TokenStrings, ValidatedTokens};
use crate::{
    metrics::client_metrics::MetricsCache,
    types::{BatchMetricsRequestBody, EdgeResult},
};

#[post("/validate")]
async fn validate(
    token_provider: web::Data<RwLock<dyn EdgeSource>>,
    tokens: Json<TokenStrings>,
) -> EdgeJsonResult<ValidatedTokens> {
    let valid_tokens = token_provider
        .read()
        .await
        .filter_valid_tokens(tokens.into_inner().tokens)
        .await?;
    Ok(Json(ValidatedTokens {
        tokens: valid_tokens,
    }))
}

#[post("/metrics")]
async fn metrics(
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
