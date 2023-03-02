use super::unleash_client::UnleashClient;
use std::time::Duration;

use crate::metrics::client_metrics::MetricsCache;
use crate::types::BatchMetricsRequestBody;
use std::sync::Arc;
use tracing::warn;

pub async fn send_metrics_task(
    metrics_cache: Arc<MetricsCache>,
    unleash_client: Arc<UnleashClient>,
    send_interval: u64,
) {
    loop {
        let metrics = metrics_cache.get_unsent_metrics();
        let body = BatchMetricsRequestBody {
            applications: metrics.applications,
            metrics: metrics.metrics,
        };

        if let Err(error) = unleash_client.send_batch_metrics(body).await {
            warn!("Failed to send metrics: {error:?}");
        } else {
            metrics_cache.reset_metrics();
        }
        tokio::time::sleep(Duration::from_secs(send_interval)).await;
    }
}
