use super::unleash_client::UnleashClient;
use std::time::Duration;

use crate::metrics::client_metrics::MetricsCache;
use crate::types::BatchMetricsRequest;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::warn;

pub async fn send_metrics_task(
    metrics_cache: Arc<RwLock<MetricsCache>>,
    unleash_client: UnleashClient,
    send_interval: u64,
) {
    loop {
        {
            let mut metrics_lock = metrics_cache.write().await;
            let metrics = metrics_lock.get_unsent_metrics();

            let request = BatchMetricsRequest {
                applications: todo!(),
                metrics: todo!(),
            };

            if let Err(error) = unleash_client.send_batch_metrics(request).await {
                warn!("Failed to send metrics match");
            } else {
                metrics_lock.reset_metrics();
            }
        }

        tokio::time::sleep(Duration::from_secs(send_interval)).await;
    }
}
