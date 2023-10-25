use actix_web::http::StatusCode;
use tracing::{error, warn};

use super::unleash_client::UnleashClient;
use std::time::Duration;

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
    loop {
        let batches = metrics_cache.get_appropriately_sized_batches();
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
        tokio::time::sleep(Duration::from_secs(send_interval)).await;
    }
}
