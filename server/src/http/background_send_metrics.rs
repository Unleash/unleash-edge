use actix_web::http::StatusCode;
use std::cmp::max;
use tracing::{error, info, trace, warn};

use super::unleash_client::UnleashClient;
use std::time::Duration;

use crate::{
    error::EdgeError,
    metrics::client_metrics::{size_of_batch, MetricsCache},
};
use lazy_static::lazy_static;
use prometheus::{register_int_gauge, register_int_gauge_vec, IntGauge, IntGaugeVec, Opts};
use rand::Rng;
use std::sync::Arc;

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
}

pub async fn send_metrics_task(
    metrics_cache: Arc<MetricsCache>,
    unleash_client: Arc<UnleashClient>,
    send_interval: u64,
) {
    let mut failures = 0;
    let mut interval = Duration::from_secs(send_interval);
    loop {
        let batches = metrics_cache.get_appropriately_sized_batches();
        trace!("Posting {} batches", batches.len());
        for batch in batches {
            if !batch.applications.is_empty() || !batch.metrics.is_empty() {
                if let Err(edge_error) = unleash_client.send_batch_metrics(batch.clone()).await {
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
                                    interval = new_interval(send_interval, failures, 5);
                                    error!("Upstream said we are trying to post to an endpoint that doesn't exist. backing off to {} seconds", interval.as_secs());
                                }
                                StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED => {
                                    failures = 10;
                                    interval = new_interval(send_interval, failures, 5);
                                    error!("Upstream said we were not allowed to post metrics, backing off to {} seconds", interval.as_secs());
                                }
                                StatusCode::TOO_MANY_REQUESTS => {
                                    failures = max(10, failures + 1);
                                    interval = new_interval(send_interval, failures, 5);
                                    info!(
                                        "Upstream said it was too busy, backing off to {} seconds",
                                        interval.as_secs()
                                    );
                                    metrics_cache.reinsert_batch(batch);
                                }
                                StatusCode::INTERNAL_SERVER_ERROR
                                | StatusCode::BAD_GATEWAY
                                | StatusCode::SERVICE_UNAVAILABLE
                                | StatusCode::GATEWAY_TIMEOUT => {
                                    failures = max(10, failures + 1);
                                    interval = new_interval(send_interval, failures, 5);
                                    info!("Upstream said it is struggling. It returned Http status {}. Backing off to {} seconds", status_code, interval.as_secs());
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
                    interval = new_interval(send_interval, failures, 5);
                }
            }
        }
        trace!(
            "Done posting traces. Sleeping for {} seconds and then going again",
            interval.as_secs()
        );
        tokio::time::sleep(interval).await;
    }
}

fn new_interval(send_interval: u64, failures: u64, max_jitter_seconds: u64) -> Duration {
    let initial = Duration::from_secs(send_interval);
    let added_interval_from_failure = Duration::from_secs(send_interval * failures);
    let jitter = random_jitter_milliseconds(max_jitter_seconds);
    initial + added_interval_from_failure + jitter
}

fn random_jitter_milliseconds(max_jitter_seconds: u64) -> Duration {
    let mut rng = rand::thread_rng();
    let jitter = rng.gen_range(0..(max_jitter_seconds * 1000));
    Duration::from_millis(jitter)
}
