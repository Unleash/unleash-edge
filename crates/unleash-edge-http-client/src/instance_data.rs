use reqwest::{StatusCode, Url};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::{ClientMetaInformation, UnleashClient};
use prometheus::Registry;
use tracing::{debug, warn};
use unleash_edge_cli::{CliArgs, EdgeMode};
use unleash_edge_types::errors::EdgeError;
use unleash_edge_types::metrics::instance_data::EdgeInstanceData;

#[derive(Debug, Clone)]
pub struct InstanceDataSender {
    pub unleash_client: Arc<UnleashClient>,
    pub registry: Registry,
    pub token: String,
    pub base_path: String,
}

#[derive(Debug, Clone)]
pub enum InstanceDataSending {
    SendNothing,
    SendInstanceData(InstanceDataSender),
}

impl InstanceDataSending {
    pub fn from_args(
        args: CliArgs,
        client_meta_information: &ClientMetaInformation,
        http_client: reqwest::Client,
        registry: Registry,
    ) -> Result<Self, EdgeError> {
        match args.mode {
            EdgeMode::Edge(edge_args) => edge_args
                .tokens
                .first()
                .map(|token| {
                    let unleash_client = Url::parse(&edge_args.upstream_url.clone())
                        .map(|url| {
                            UnleashClient::from_url_with_backing_client(
                                url,
                                args.auth_headers
                                    .upstream_auth_header
                                    .clone()
                                    .unwrap_or("Authorization".to_string()),
                                http_client,
                                client_meta_information.clone(),
                            )
                        })
                        .map(|c| {
                            c.with_custom_client_headers(edge_args.custom_client_headers.clone())
                        })
                        .map(Arc::new)
                        .map_err(|_| EdgeError::InvalidServerUrl(edge_args.upstream_url.clone()))
                        .expect("Could not construct UnleashClient");
                    let instance_data_sender = InstanceDataSender {
                        unleash_client,
                        token: token.clone(),
                        base_path: args.http.base_path.clone(),
                        registry,
                    };
                    InstanceDataSending::SendInstanceData(instance_data_sender)
                })
                .map(Ok)
                .unwrap_or(Ok(InstanceDataSending::SendNothing)),
            _ => Ok(InstanceDataSending::SendNothing),
        }
    }
}

pub async fn send_instance_data(
    instance_data_sender: &InstanceDataSender,
    our_instance_data: Arc<EdgeInstanceData>,
    downstream_instance_data: Arc<RwLock<Vec<EdgeInstanceData>>>,
) -> Result<(), EdgeError> {
    let observed_data = our_instance_data.observe(
        &instance_data_sender.registry,
        downstream_instance_data.read().await.clone(),
        &instance_data_sender.base_path,
    );
    instance_data_sender
        .unleash_client
        .post_edge_observability_data(observed_data, &instance_data_sender.token)
        .await
}
pub async fn loop_send_instance_data(
    instance_data_sender: Arc<InstanceDataSending>,
    our_instance_data: Arc<EdgeInstanceData>,
    downstream_instance_data: Arc<RwLock<Vec<EdgeInstanceData>>>,
) {
    let mut errors = 0;
    let delay = std::time::Duration::from_secs(60);
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(60) + delay * std::cmp::min(errors, 10))
            .await;
        match instance_data_sender.as_ref() {
            InstanceDataSending::SendNothing => {
                debug!("No instance data sender found. Doing nothing.");
                continue;
            }
            InstanceDataSending::SendInstanceData(instance_data_sender) => {
                let status = send_instance_data(
                    instance_data_sender,
                    our_instance_data.clone(),
                    downstream_instance_data.clone(),
                )
                .await;
                if let Err(e) = status {
                    match e {
                        EdgeError::EdgeMetricsRequestError(status, _) => {
                            if status == StatusCode::NOT_FOUND {
                                debug!(
                                    "Our upstream is not running a version that supports edge metrics."
                                );
                                errors += 1;
                                downstream_instance_data.write().await.clear();
                                our_instance_data.clear_time_windowed_metrics();
                            } else if status == StatusCode::FORBIDDEN {
                                warn!(
                                    "Upstream edge metrics said our token wasn't allowed to post data"
                                );
                                errors += 1;
                                downstream_instance_data.write().await.clear();
                                our_instance_data.clear_time_windowed_metrics();
                            }
                        }
                        _ => {
                            warn!("Failed to post instance data due to unknown error {e:?}");
                        }
                    }
                } else {
                    debug!("Successfully posted observability metrics.");
                    errors = 0;
                    downstream_instance_data.write().await.clear();
                    our_instance_data.clear_time_windowed_metrics();
                }
            }
        }
    }
}
