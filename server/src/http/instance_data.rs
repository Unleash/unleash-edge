use reqwest::StatusCode;
use std::sync::Arc;
use tokio::sync::RwLock;

use prometheus::Registry;
use tracing::{debug, info, trace, warn};

use crate::metrics::edge_metrics::EdgeInstanceData;

use super::refresher::feature_refresher::FeatureRefresher;

pub async fn send_instance_data(
    feature_refresher: Arc<FeatureRefresher>,
    prometheus_registry: Registry,
    our_instance_data: Arc<EdgeInstanceData>,
    downstream_instance_data: Arc<RwLock<Vec<EdgeInstanceData>>>,
) {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(15)).await;
        trace!("Looping instance data sending");
        let downstream_instances = downstream_instance_data.read().await.clone();
        let observed_data = our_instance_data.observe(&prometheus_registry, downstream_instances);
        info!("Observed when sending: {observed_data:?}");
        let status = feature_refresher
            .unleash_client
            .send_instance_data(
                observed_data,
                &feature_refresher
                    .tokens_to_refresh
                    .iter()
                    .next()
                    .map(|t| t.value().clone())
                    .map(|t| t.token.token.clone())
                    .expect("No token to refresh, cowardly panic'ing"),
            )
            .await;
        match status {
            Ok(_) => {
                info!("Upstream successfully accepted our data");
                {
                    downstream_instance_data.write().await.clear();
                    info!("cleared downstream instances")
                }
            }
            Err(e) => match e {
                crate::error::EdgeError::EdgeMetricsRequestError(status, _message) => {
                    warn!("Failed to post instance data with status {status}");
                    if status == StatusCode::NOT_FOUND {
                        debug!("Upstream edge metrics not found, clearing our data about downstream instances to avoid growing to infinity (and beyond!)");
                        {
                            downstream_instance_data.write().await.clear();
                            info!("cleared downstream instances")
                        }
                    } else if status == StatusCode::FORBIDDEN {
                        warn!("Upstream edge metrics rejected our data, clearing our data about downstream instances to avoid growing to infinity (and beyond!)");
                        {
                            downstream_instance_data.write().await.clear();
                            info!("cleared downstream instances")
                        }
                    }
                }
                _ => {
                    warn!("Failed to post instance data due to unknown error {e:?}");
                }
            },
        }
    }
}
