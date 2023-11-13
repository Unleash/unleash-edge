use actix_web::http::StatusCode;
use tracing::{error, info, warn};

use super::unleash_client::UnleashClient;
use std::{
    cmp::{max, min},
    time::Duration,
};

use crate::{
    error::EdgeError,
    metrics::client_metrics::{size_of_batch, MetricsCache},
};
use lazy_static::lazy_static;
use prometheus::{register_int_gauge, register_int_gauge_vec, IntGauge, IntGaugeVec, Opts};
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
    let maximum_backoff: u32 = max(1, 300 / send_interval.try_into().unwrap());
    let mut failures: u32 = 0;
    let mut skips: u32 = 0;
    loop {
        info!(failures, skips, maximum_backoff);
        if skips == 0 {
            let batches = metrics_cache.get_appropriately_sized_batches();
            for batch in batches {
                if !batch.applications.is_empty() || !batch.metrics.is_empty() {
                    if let Err(edge_error) = unleash_client.send_batch_metrics(batch.clone()).await
                    {
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
                                        failures += 1;
                                        skips = maximum_backoff;
                                    }
                                    StatusCode::NOT_FOUND => {
                                        error!("Unleash said that [{}] did not exist. Dropping this metric bucket", unleash_client.urls.edge_metrics_url);
                                        failures += 1;
                                        skips = maximum_backoff;
                                    }
                                    StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                                        error!("Unleash said that we were not allowed to send metrics (Status code: {status_code}). Dropping this metric bucket");
                                        failures += 1;
                                        skips = maximum_backoff;
                                    }
                                    StatusCode::TOO_MANY_REQUESTS => {
                                        error!(
                                            "Unleash said that we were sending too many requests."
                                        );
                                        failures += 1;
                                        skips = min(skips + 1, maximum_backoff);
                                        metrics_cache.reinsert_batch(batch);
                                    }
                                    StatusCode::INTERNAL_SERVER_ERROR
                                    | StatusCode::BAD_GATEWAY
                                    | StatusCode::SERVICE_UNAVAILABLE
                                    | StatusCode::GATEWAY_TIMEOUT => {
                                        error!("Unleash said that it was having issues (Status code: {status_code})");
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
                    }
                }
            }
            failures = max(0, failures - 1);
            skips = max(failures, 0);
        } else {
            skips = max(0, skips - 1);
        }
        tokio::time::sleep(Duration::from_secs(send_interval)).await;
    }
}
