use std::sync::{Arc, RwLock};

use prometheus::Registry;
use tracing::{debug, info, trace};

use crate::metrics::edge_metrics::EdgeInstanceData;

use super::refresher::feature_refresher::FeatureRefresher;

pub async fn send_instance_data(
    feature_refresher: Arc<FeatureRefresher>,
    prometheus_registry: &Registry,
    our_instance_data: Arc<EdgeInstanceData>,
    downstream_instance_data: Arc<RwLock<Vec<EdgeInstanceData>>>,
) {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(15)).await;
        trace!("Looping instance data sending");
        let mut observed_data = our_instance_data.observe(prometheus_registry);
        {
            let downstream_instance_data = downstream_instance_data.read().unwrap().clone();
            for downstream in downstream_instance_data {
                observed_data = observed_data.add_downstream(downstream);
            }
        }
        {
            downstream_instance_data.write().unwrap().clear();
        }
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
            Ok(_) => info!("Posted instance data"),
            Err(_) => info!("Failed to post instance data"),
        }
    }
}
