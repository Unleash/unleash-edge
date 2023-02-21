use super::unleash_client::UnleashClient;
use std::time::Duration;

use crate::error::EdgeError;
use crate::metrics::client_metrics::MetricsCache;
use crate::types::{
    BatchMetricsRequest, BatchMetricsRequestBody, EdgeResult, EdgeSource, EdgeToken,
};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::warn;

pub async fn send_metrics_task(
    metrics_cache: Arc<RwLock<MetricsCache>>,
    source: Arc<dyn EdgeSource>,
    unleash_client: UnleashClient,
    send_interval: u64,
) {
    loop {
        {
            let mut metrics_lock = metrics_cache.write().await;
            let metrics = metrics_lock.get_unsent_metrics();
            let api_key = get_first_token(source.clone()).await;

            match api_key {
                Ok(api_key) => {
                    let request = BatchMetricsRequest {
                        api_key: api_key.token.clone(),
                        body: BatchMetricsRequestBody {
                            applications: metrics.applications,
                            metrics: metrics.metrics,
                        },
                    };

                    if let Err(error) = unleash_client.send_batch_metrics(request).await {
                        warn!("Failed to send metrics: {error:?}");
                    } else {
                        metrics_lock.reset_metrics();
                    }
                }
                Err(e) => {
                    warn!("Error sending metrics: {e:?}")
                }
            }
        }

        tokio::time::sleep(Duration::from_secs(send_interval)).await;
    }
}

async fn get_first_token(source: Arc<dyn EdgeSource>) -> EdgeResult<EdgeToken> {
    let api_key = source.get_valid_tokens().await?.get(0).cloned();
    match api_key {
        Some(api_key) => Ok(api_key),
        None => Err(EdgeError::DataSourceError("No tokens found".into())),
    }
}
