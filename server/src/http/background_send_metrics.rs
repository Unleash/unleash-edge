use std::cmp::max;
use std::sync::Arc;

use chrono::Duration;
use dashmap::DashMap;
use lazy_static::lazy_static;
use prometheus::{register_int_gauge, register_int_gauge_vec, IntGauge, IntGaugeVec, Opts};
use reqwest::StatusCode;
use tracing::{error, info, trace, warn};

use crate::types::TokenRefresh;
use crate::{
    error::EdgeError,
    metrics::client_metrics::{size_of_batch, MetricsCache},
};

use super::refresher::feature_refresher::FeatureRefresher;

lazy_static! {
    pub static ref METRICS_UPSTREAM_HTTP_ERRORS: IntGaugeVec = register_int_gauge_vec!(
        Opts::new(
            "metrics_upstream_http_errors",
            "Failing requests against upstream metrics endpoint"
        ),
        &["status_code"]
    )
    .unwrap();
    pub static ref METRICS_UNEXPECTED_ERRORS: IntGauge =
        register_int_gauge!(Opts::new("metrics_send_error", "Failures to send metrics")).unwrap();
    pub static ref METRICS_UPSTREAM_OUTDATED: IntGaugeVec = register_int_gauge_vec!(
        Opts::new(
            "metrics_upstream_outdated",
            "Number of times we have tried to send metrics to an outdated endpoint"
        ),
        &["environment"]
    )
    .unwrap();
    pub static ref METRICS_UPSTREAM_CLIENT_BULK: IntGaugeVec = register_int_gauge_vec!(
        Opts::new(
            "metrics_upstream_client_bulk",
            "Number of times we have tried to send metrics to the client bulk endpoint"
        ),
        &["environment"]
    )
    .unwrap();
    pub static ref METRICS_INTERVAL_BETWEEN_SEND: IntGauge = register_int_gauge!(Opts::new(
        "metrics_interval_between_send",
        "Interval between sending metrics"
    ))
    .unwrap();
}

fn decide_where_to_post(
    environment: &String,
    known_tokens: Arc<DashMap<String, TokenRefresh>>,
) -> (bool, String) {
    if let Some(token_refresh) = known_tokens
        .iter()
        .find(|t| t.token.environment == Some(environment.to_string()))
    {
        METRICS_UPSTREAM_CLIENT_BULK
            .with_label_values(&[environment])
            .inc();
        (true, token_refresh.token.token.clone())
    } else {
        (false, "".into())
    }
}

pub async fn send_metrics_one_shot(
    metrics_cache: Arc<MetricsCache>,
    feature_refresher: Arc<FeatureRefresher>,
) {
    let envs = metrics_cache.get_metrics_by_environment();
    for (env, batch) in envs.iter() {
        let (use_new_endpoint, token) =
            decide_where_to_post(env, feature_refresher.tokens_to_refresh.clone());
        let batches = metrics_cache.get_appropriately_sized_env_batches(batch);
        trace!("Posting {} batches for {env}", batches.len());
        for batch in batches {
            if !batch.applications.is_empty() || !batch.metrics.is_empty() {
                let result = if use_new_endpoint {
                    feature_refresher
                        .unleash_client
                        .send_bulk_metrics_to_client_endpoint(batch.clone(), &token)
                        .await
                } else {
                    feature_refresher
                        .unleash_client
                        .send_batch_metrics(batch.clone(), None)
                        .await
                };
                if let Err(edge_error) = result {
                    warn!("Shut down metrics flush failed with {edge_error:?}")
                }
            }
        }
    }
}

pub async fn send_metrics_task(
    metrics_cache: Arc<MetricsCache>,
    feature_refresher: Arc<FeatureRefresher>,
    send_interval: i64,
) {
    let mut failures = 0;
    let mut interval = Duration::seconds(send_interval);
    loop {
        trace!("Looping metrics");
        let envs = metrics_cache.get_metrics_by_environment();
        for (env, batch) in envs.iter() {
            let (use_new_endpoint, token) =
                decide_where_to_post(env, feature_refresher.tokens_to_refresh.clone());
            let batches = metrics_cache.get_appropriately_sized_env_batches(batch);
            trace!("Posting {} batches for {env}", batches.len());
            for batch in batches {
                if !batch.applications.is_empty() || !batch.metrics.is_empty() {
                    let result = if use_new_endpoint {
                        feature_refresher
                            .unleash_client
                            .send_bulk_metrics_to_client_endpoint(batch.clone(), &token)
                            .await
                    } else {
                        feature_refresher
                            .unleash_client
                            .send_batch_metrics(batch.clone(), Some(interval.num_milliseconds()))
                            .await
                    };
                    if let Err(edge_error) = result {
                        match edge_error {
                            EdgeError::EdgeMetricsRequestError(status_code, message) => {
                                METRICS_UPSTREAM_HTTP_ERRORS
                                    .with_label_values(&[status_code.as_str()])
                                    .inc();
                                match status_code {
                                    StatusCode::PAYLOAD_TOO_LARGE => error!(
                                        "Metrics were too large. They were {}",
                                        size_of_batch(&batch)
                                    ),
                                    StatusCode::BAD_REQUEST => {
                                        error!("Unleash said [{message:?}]. Dropping this metric bucket to avoid consuming too much memory");
                                    }
                                    StatusCode::NOT_FOUND => {
                                        failures = 10;
                                        interval = new_interval(send_interval, failures);
                                        error!("Upstream said we are trying to post to an endpoint that doesn't exist. backing off to {} seconds", interval.num_seconds());
                                    }
                                    StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED => {
                                        failures = 10;
                                        interval = new_interval(send_interval, failures);
                                        error!("Upstream said we were not allowed to post metrics, backing off to {} seconds", interval.num_seconds());
                                    }
                                    StatusCode::TOO_MANY_REQUESTS => {
                                        failures = max(10, failures + 1);
                                        interval = new_interval(send_interval, failures);
                                        info!(
                                            "Upstream said it was too busy, backing off to {} seconds",
                                            interval.num_seconds()
                                        );
                                        metrics_cache.reinsert_batch(batch);
                                    }
                                    StatusCode::INTERNAL_SERVER_ERROR
                                    | StatusCode::BAD_GATEWAY
                                    | StatusCode::SERVICE_UNAVAILABLE
                                    | StatusCode::GATEWAY_TIMEOUT => {
                                        failures = max(10, failures + 1);
                                        interval = new_interval(send_interval, failures);
                                        info!("Upstream said it is struggling. It returned Http status {}. Backing off to {} seconds", status_code, interval.num_seconds());
                                        metrics_cache.reinsert_batch(batch);
                                    }
                                    _ => {
                                        warn!("Failed to send metrics. Status code was {status_code}. Will reinsert metrics for next attempt");
                                        metrics_cache.reinsert_batch(batch);
                                    }
                                }
                            }
                            _ => {
                                warn!("Failed to send metrics: {edge_error:?}");
                                METRICS_UNEXPECTED_ERRORS.inc();
                            }
                        }
                    } else {
                        failures = max(0, failures - 1);
                        interval = new_interval(send_interval, failures);
                    }
                }
            }
        }
        trace!(
            "Done posting traces. Sleeping for {} seconds and then going again",
            interval.num_seconds()
        );
        METRICS_INTERVAL_BETWEEN_SEND.set(interval.num_seconds());
        tokio::time::sleep(std::time::Duration::from_secs(interval.num_seconds() as u64)).await;
    }
}

fn new_interval(send_interval: i64, failures: i64) -> Duration {
    let added_interval_from_failure = send_interval * failures;
    Duration::seconds(send_interval + added_interval_from_failure)
}

#[cfg(test)]
mod tests {
    use crate::http::background_send_metrics::new_interval;

    #[tokio::test]
    pub async fn new_interval_does_not_overflow() {
        let metrics = new_interval(300, 10);
        assert!(metrics.num_seconds() < 3305);
    }
}
