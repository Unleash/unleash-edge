use actix_web::{
    get, post,
    web::{self, Json},
    HttpResponse,
};
use tokio::sync::RwLock;
use unleash_types::client_metrics::MetricBucket;

use crate::types::{
    BatchMetricsRequest, EdgeJsonResult, EdgeSource, EdgeToken, TokenStrings, ValidatedTokens,
};
use crate::{
    metrics::client_metrics::MetricsCache, metrics::client_metrics::MetricsKey, types::EdgeResult,
};

#[get("/validate")]
async fn validate(
    _client_token: EdgeToken,
    token_provider: web::Data<RwLock<dyn EdgeSource>>,
    tokens: Json<TokenStrings>,
) -> EdgeJsonResult<ValidatedTokens> {
    let valid_tokens = token_provider
        .read()
        .await
        .get_valid_tokens(tokens.into_inner().tokens)
        .await?;
    Ok(Json(ValidatedTokens {
        tokens: valid_tokens,
    }))
}

#[post("/metrics")]
async fn metrics(
    _client_token: EdgeToken,
    batch_metrics_request: web::Json<BatchMetricsRequest>,
    metrics_cache: web::Data<RwLock<MetricsCache>>,
) -> EdgeResult<HttpResponse> {
    {
        let mut metrics_lock = metrics_cache.write().await;

        for metric in batch_metrics_request.metrics.iter() {
            let key = if let Some(instance_id) = &metric.instance_id {
                MetricsKey {
                    app_name: metric.app_name.clone(),
                    instance_id: instance_id.clone(),
                }
            } else {
                MetricsKey::from_app_name(metric.app_name.clone())
            };
            metrics_lock
                .metrics
                .entry(key)
                .and_modify(|to_modify| {
                    to_modify
                        .toggles
                        .entry(metric.feature_name)
                        .and_modify(|feature_stats| {
                            feature_stats.yes += metric.yes;
                            feature_stats.no += metric.no;
                            metric.variants.into_iter().for_each(|(k, added_count)| {
                                feature_stats
                                    .variants
                                    .entry(k)
                                    .and_modify(|count| {
                                        *count += added_count;
                                    })
                                    .or_insert(added_count);
                            });
                        });
                })
                .or_insert(MetricBucket {
                    start: metric.timestamp,
                });
        }
    }
    Ok(HttpResponse::Accepted().finish())
}

pub fn configure_edge_api(cfg: &mut web::ServiceConfig) {
    cfg.service(validate);
}
