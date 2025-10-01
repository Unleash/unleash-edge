use std::collections::HashMap;
use std::sync::Arc;
use std::{cmp::max, pin::Pin};

use chrono::Duration;
use futures::{StreamExt, stream};
use lazy_static::lazy_static;
use prometheus::{IntGauge, IntGaugeVec, Opts, register_int_gauge, register_int_gauge_vec};
use rand::{Rng, rng};
use reqwest::StatusCode;
use tracing::{debug, error, info, trace, warn};
use unleash_edge_types::metrics::batching::MetricsBatch;
use unleash_edge_types::tokens::EdgeToken;

use unleash_edge_http_client::UnleashClient;
use unleash_edge_types::{errors::EdgeError, metrics::MetricsCache};

const MAX_INFLIGHT_PER_ENV: usize = 16;
const MAX_RETRIES: usize = 5;
const BASE_BACKOFF_MS: u64 = 50;

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

#[derive(Debug)]
enum MetricsSendError {
    NoBackoff(String),
    Backoff(String),
    Unauthed(String),
    Unknown(String),
}

async fn send_one_with_retry(
    unleash_client: &UnleashClient,
    token: &str,
    batch: MetricsBatch,
    metrics_cache: &MetricsCache,
) -> Result<(), MetricsSendError> {
    let mut attempt = 0usize;

    loop {
        if batch.applications.is_empty()
            && batch.metrics.is_empty()
            && batch.impact_metrics.is_empty()
        {
            return Ok(());
        }

        match unleash_client
            .send_bulk_metrics_to_client_endpoint(batch.clone(), token)
            .await
        {
            Ok(()) => return Ok(()),

            Err(EdgeError::EdgeMetricsRequestError(status, msg)) => {
                METRICS_UPSTREAM_HTTP_ERRORS
                    .with_label_values(&[status.as_str()])
                    .inc();
                return match status {
                    StatusCode::PAYLOAD_TOO_LARGE => Err(MetricsSendError::NoBackoff(format!(
                        "Metrics were too large. They were {}. Dropping this bucket to avoid consuming too much memory",
                        size_of_batch(&batch)
                    ))),
                    StatusCode::BAD_REQUEST => Err(MetricsSendError::NoBackoff(format!(
                        "Unleash said [{msg:?}]. Dropping this bucket to avoid consuming too much memory"
                    ))),

                    StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED | StatusCode::NOT_FOUND => {
                        match msg {
                            Some(m) => Err(MetricsSendError::Unauthed(format!("Upstream said we were not allowed to post metrics. It replied ({:?}). Dropping this bucket to avoid consuming too much memory", m))),
                            None => Err(MetricsSendError::Unauthed("Upstream said we were not allowed to post metrics without a message. Dropping this bucket to avoid consuming too much memory".to_string()))
                        }
                    }

                    StatusCode::TOO_MANY_REQUESTS
                    | StatusCode::INTERNAL_SERVER_ERROR
                    | StatusCode::BAD_GATEWAY
                    | StatusCode::SERVICE_UNAVAILABLE
                    | StatusCode::GATEWAY_TIMEOUT => {
                        reinsert_batch(metrics_cache, batch);
                        Err(MetricsSendError::Backoff(format!(
	                    "Upstream said it is struggling. It returned Http status {}",
                            status
                        )))
                    }

                    _ => {
                        reinsert_batch(metrics_cache, batch);
                        Err(MetricsSendError::Unknown(format!(
                            "Upstream returned an unexpected status code: {}",
                            status
                        )))
                    }
                };
            }

            Err(other) => {
                // This arm is network level failures, timeouts, etc. In this case we're gonna slap some jitter on it and try
                // again in place before allowing the higher level backoff to kick in. Importantly this doesn't block the whole
                // queue, since we have MAX_INFLIGHT_PER_ENV requests in flight at once. If all MAX_INFLIGHT_PER_ENV are sleeping,
                // then we do actually need to chill out a little more to avoid thrashing anyway.
                if attempt >= MAX_RETRIES {
                    reinsert_batch(metrics_cache, batch);
                    return Err(MetricsSendError::Unknown(format!(
                        "Network failure after {} retries: {other:?}; reinserted",
                        attempt
                    )));
                }
                let backoff = (BASE_BACKOFF_MS << attempt).min(2000);
                let jitter: u64 = rng().random_range(0..=backoff / 5);
                tokio::time::sleep(tokio::time::Duration::from_millis(backoff + jitter)).await;
                attempt += 1;
                continue;
            }
        }
    }
}

async fn send_metrics(
    envs: HashMap<String, MetricsBatch>,
    unleash_client: Arc<UnleashClient>,
    metrics_cache: Arc<MetricsCache>,
    startup_tokens: Vec<EdgeToken>,
) -> Vec<Result<(), MetricsSendError>> {
    let mut results = Vec::new();

    for (env, batch) in envs {
        let slices = get_appropriately_sized_env_batches(&metrics_cache, &batch);

        tracing::trace!("Posting {} batches for {}", slices.len(), env);

        let stream = stream::iter(slices.into_iter().map(|slice| {
            let client = unleash_client.clone();
            let cache = metrics_cache.clone();
            let tok = startup_tokens
                .iter()
                .find(|t| t.environment == Some(env.clone()))
                .map(|t| t.token.clone())
                .expect("Unable to determine token to use for metrics sending");
            async move { send_one_with_retry(&client, &tok, slice, &cache).await }
        }));

        let mut buffered = stream.buffered(MAX_INFLIGHT_PER_ENV);

        while let Some(res) = buffered.next().await {
            results.push(res);
        }
    }
    results
}

pub fn create_once_off_send_metrics(
    metrics_cache: Arc<MetricsCache>,
    unleash_client: Arc<UnleashClient>,
    startup_tokens: Vec<EdgeToken>,
) -> Pin<Box<dyn Future<Output = ()> + Send>> {
    let metrics_cache = metrics_cache.clone();
    let unleash_client = unleash_client.clone();

    Box::pin(async move {
        let envs = get_metrics_by_environment(&metrics_cache);
        let results = send_metrics(
            envs,
            unleash_client.clone(),
            metrics_cache.clone(),
            startup_tokens,
        )
        .await;
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
    startup_tokens: Vec<EdgeToken>,
    send_interval: i64,
) -> Pin<Box<dyn Future<Output = ()> + Send>> {
    let mut failures = 0;
    let mut interval = Duration::seconds(send_interval);
    Box::pin(async move {
        loop {
            debug!("Looping metrics");
            let envs = get_metrics_by_environment(&metrics_cache);

            let results = send_metrics(
                envs,
                unleash_client.clone(),
                metrics_cache.clone(),
                startup_tokens.clone(),
            )
            .await;

            debug!("Done sending metrics, got {} results", results.len());

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
