use std::collections::HashMap;
use std::sync::Arc;
use std::{cmp::max, pin::Pin};

use chrono::Duration;
use lazy_static::lazy_static;
use prometheus::{IntGauge, IntGaugeVec, Opts, register_int_gauge, register_int_gauge_vec};
use reqwest::StatusCode;
use tracing::{error, info, trace, warn};
use unleash_edge_types::metrics::batching::MetricsBatch;
use unleash_edge_types::{TokenCache, TokenValidationStatus, tokens::EdgeToken};

use unleash_edge_http_client::UnleashClient;
use unleash_edge_types::{errors::EdgeError, metrics::MetricsCache};

use crate::{
    client_metrics::{
        get_appropriately_sized_env_batches, get_metrics_by_environment, reinsert_batch,
    },
    metric_batching::size_of_batch,
};

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

fn get_valid_token(token_cache: Arc<TokenCache>) -> Option<EdgeToken> {
    token_cache
        .iter()
        .find(|token| {
            token.status == TokenValidationStatus::Validated
                || token.status == TokenValidationStatus::Trusted
        })
        .map(|t| t.clone())
}

#[derive(Debug)]
enum MetricsSendError {
    NoBackoff(String),
    Backoff(String),
    Unauthed(String),
    Unknown(String),
}

async fn send_metrics(
    envs: HashMap<String, MetricsBatch>,
    unleash_client: Arc<UnleashClient>,
    metrics_cache: Arc<MetricsCache>,
    token: &EdgeToken,
) -> Vec<Result<(), MetricsSendError>> {
    let mut results = vec![];
    for (env, batch) in envs.iter() {
        let batches = get_appropriately_sized_env_batches(&metrics_cache, batch);
        trace!("Posting {} batches for {env}", batches.len());
        for batch in batches {
            if !batch.applications.is_empty()
                || !batch.metrics.is_empty()
                || !batch.impact_metrics.is_empty()
            {
                let result = unleash_client
                    .send_bulk_metrics_to_client_endpoint(batch.clone(), &token.token)
                    .await
                    .map_err(|edge_error| match edge_error {
                        EdgeError::EdgeMetricsRequestError(status_code, message) => {
                            METRICS_UPSTREAM_HTTP_ERRORS
                                .with_label_values(&[status_code.as_str()])
                                .inc();
                            match status_code {
                                StatusCode::PAYLOAD_TOO_LARGE => {
                                    MetricsSendError::NoBackoff(format!(
                                        "Metrics were too large. They were {}. Dropping this bucket to avoid consuming too much memory",
                                        size_of_batch(&batch)
                                    ))
                                }
                                StatusCode::BAD_REQUEST => {
                                    MetricsSendError::NoBackoff(format!(
                                        "Unleash said [{message:?}]. Dropping this bucket to avoid consuming too much memory"
                                    ))
                                }
                                StatusCode::NOT_FOUND => {
                                    MetricsSendError::Backoff("Upstream said we are trying to post to an endpoint that doesn't exist. Dropping this bucket to avoid consuming too much memory".to_string())
                                }
                                StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED => {
                                    MetricsSendError::Unauthed("Upstream said we were not allowed to post metrics. Dropping this bucket to avoid consuming too much memory".to_string())
                                }
                                StatusCode::TOO_MANY_REQUESTS => {
                                    reinsert_batch(&metrics_cache, batch);
                                    MetricsSendError::Backoff("Upstream said it was too busy".to_string())
                                }
                                StatusCode::INTERNAL_SERVER_ERROR
                                | StatusCode::BAD_GATEWAY
                                | StatusCode::SERVICE_UNAVAILABLE
                                | StatusCode::GATEWAY_TIMEOUT => {
                                    reinsert_batch(&metrics_cache, batch);
                                    MetricsSendError::Backoff(format!(
                                        "Upstream said it is struggling. It returned Http status {}",
                                        status_code
                                    ))
                                }
                                _ => {
                                    reinsert_batch(&metrics_cache, batch);
                                    MetricsSendError::Unknown(format!(
                                        "Upstream returned an unexpected status code: {}",
                                        status_code
                                    ))
                                }
                            }
                        }
                        _ => {
                            MetricsSendError::Unknown(format!(
                                "Failed to send metrics: {edge_error:?}. Dropping this bucket to avoid consuming too much memory"
                            ))
                        }
                    });
                results.push(result);
            }
        }
    }
    results
}

pub fn create_once_off_send_metrics(
    metrics_cache: Arc<MetricsCache>,
    unleash_client: Arc<UnleashClient>,
    token_cache: Arc<TokenCache>,
) -> Pin<Box<dyn Future<Output = ()> + Send>> {
    let metrics_cache = metrics_cache.clone();
    let unleash_client = unleash_client.clone();
    let token_cache = token_cache.clone();

    Box::pin(async move {
        let token = get_valid_token(token_cache.clone());
        let Some(token) = token else {
            warn!(
                "No valid token found for final metrics send. Shutting down without flushing metrics."
            );
            return;
        };
        let envs = get_metrics_by_environment(&metrics_cache);
        let results =
            send_metrics(envs, unleash_client.clone(), metrics_cache.clone(), &token).await;
        let errors: Vec<&MetricsSendError> =
            results.iter().filter_map(|r| r.as_ref().err()).collect();
        if !errors.is_empty() {
            warn!("Some metrics sending tasks failed during final flush: {errors:?}");
        }
    })
}

pub fn create_send_metrics_task(
    metrics_cache: Arc<MetricsCache>,
    unleash_client: Arc<UnleashClient>,
    token_cache: Arc<TokenCache>,
    send_interval: i64,
) -> Pin<Box<dyn Future<Output = ()> + Send>> {
    let mut failures = 0;
    let mut interval = Duration::seconds(send_interval);
    Box::pin(async move {
        loop {
            trace!("Looping metrics");
            let envs = get_metrics_by_environment(&metrics_cache);
            let token = get_valid_token(token_cache.clone());
            let Some(token) = token else {
                continue;
            };

            let results =
                send_metrics(envs, unleash_client.clone(), metrics_cache.clone(), &token).await;

            for result in results {
                match result {
                    Err(MetricsSendError::Unauthed(message)) => {
                        failures = 10;
                        interval = new_interval(send_interval, failures);
                        error!(
                            "{message}, backing off to {} seconds",
                            interval.num_seconds()
                        );
                    }
                    Err(MetricsSendError::Backoff(message)) => {
                        failures = max(10, failures + 1);
                        interval = new_interval(send_interval, failures);
                        info!(
                            "{message}, backing off to {} seconds",
                            interval.num_seconds()
                        );
                    }
                    Err(MetricsSendError::NoBackoff(message)) => {
                        error!("{message}");
                    }
                    Err(MetricsSendError::Unknown(message)) => {
                        warn!("Failed to send metrics: {message}");
                        METRICS_UNEXPECTED_ERRORS.inc();
                    }
                    Ok(_) => {
                        failures = max(0, failures - 1);
                        interval = new_interval(send_interval, failures);
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
    })
}

fn new_interval(send_interval: i64, failures: i64) -> Duration {
    let added_interval_from_failure = send_interval * failures;
    Duration::seconds(send_interval + added_interval_from_failure)
}

#[cfg(test)]
mod tests {
    use crate::send_unleash_metrics::new_interval;

    #[tokio::test]
    pub async fn new_interval_does_not_overflow() {
        let metrics = new_interval(300, 10);
        assert!(metrics.num_seconds() < 3305);
    }
}
