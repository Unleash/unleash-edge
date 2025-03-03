use chrono::Duration;
use reqwest::{StatusCode, Url};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::cli::{CliArgs, EdgeMode};
use crate::error::EdgeError;
use crate::http::unleash_client::{new_reqwest_client, ClientMetaInformation, UnleashClient};
use crate::metrics::edge_metrics::EdgeInstanceData;
use prometheus::Registry;
use tracing::{debug, warn};

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
        instance_data: Arc<EdgeInstanceData>,
        registry: Registry,
    ) -> Result<Self, EdgeError> {
        match args.mode {
            EdgeMode::Edge(edge_args) => {
                let identifier = instance_data.identifier.clone();
                edge_args
                    .tokens
                    .first()
                    .map(|token| {
                        let client_meta_information = ClientMetaInformation {
                            app_name: args.app_name,
                            instance_id: identifier.clone(),
                            connection_id: identifier,
                        };
                        let http_client = new_reqwest_client(
                            edge_args.skip_ssl_verification,
                            edge_args.client_identity.clone(),
                            edge_args.upstream_certificate_file.clone(),
                            Duration::seconds(edge_args.upstream_request_timeout),
                            Duration::seconds(edge_args.upstream_socket_timeout),
                            client_meta_information.clone(),
                        )
                        .expect(
                            "Could not construct reqwest client for posting observability data",
                        );
                        let unleash_client = Url::parse(&edge_args.upstream_url.clone())
                            .map(|url| {
                                UnleashClient::from_url(
                                    url,
                                    args.token_header.token_header.clone(),
                                    http_client,
                                    client_meta_information.clone(),
                                )
                            })
                            .map(|c| {
                                c.with_custom_client_headers(
                                    edge_args.custom_client_headers.clone(),
                                )
                            })
                            .map(Arc::new)
                            .map_err(|_| {
                                EdgeError::InvalidServerUrl(edge_args.upstream_url.clone())
                            })
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
                    .unwrap_or(Ok(InstanceDataSending::SendNothing))
            }
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
                                debug!("Our upstream is not running a version that supports edge metrics.");
                                errors += 1;
                                downstream_instance_data.write().await.clear();
                                our_instance_data.requests_since_last_report.clear();
                            } else if status == StatusCode::FORBIDDEN {
                                warn!("Upstream edge metrics said our token wasn't allowed to post data");
                                errors += 1;
                                downstream_instance_data.write().await.clear();
                                our_instance_data.requests_since_last_report.clear();
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
                    our_instance_data.requests_since_last_report.clear();
                }
            }
        }
    }
}
