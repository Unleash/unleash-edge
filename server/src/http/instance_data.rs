use chrono::Duration;
use reqwest::{StatusCode, Url};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::cli::{CliArgs, EdgeMode};
use crate::error::EdgeError;
use crate::http::unleash_client::{new_reqwest_client, ClientMetaInformation, UnleashClient};
use crate::metrics::edge_metrics::EdgeInstanceData;
use prometheus::Registry;
use tracing::{debug, trace, warn};

#[derive(Debug, Clone)]
pub struct InstanceDataSender {
    pub unleash_client: Arc<UnleashClient>,
    pub token: String,
}

impl InstanceDataSender {
    pub fn from_args(
        args: CliArgs,
        instance_data: Arc<EdgeInstanceData>,
    ) -> Result<Option<Self>, EdgeError> {
        match args.mode {
            EdgeMode::Edge(edge_args) => {
                let instance_id = instance_data.identifier.clone();
                Ok(edge_args.tokens.first().map(|token| {
                    let client_meta_information = ClientMetaInformation {
                        app_name: args.app_name,
                        instance_id,
                    };
                    let http_client = new_reqwest_client(
                        edge_args.skip_ssl_verification,
                        edge_args.client_identity.clone(),
                        edge_args.upstream_certificate_file.clone(),
                        Duration::seconds(edge_args.upstream_request_timeout),
                        Duration::seconds(edge_args.upstream_socket_timeout),
                        client_meta_information.clone(),
                    )
                    .expect("Could not construct reqwest client for posting observability data");
                    let unleash_client = Url::parse(&edge_args.upstream_url.clone())
                        .map(|url| {
                            UnleashClient::from_url(
                                url,
                                args.token_header.token_header.clone(),
                                http_client,
                            )
                        })
                        .map(|c| {
                            c.with_custom_client_headers(edge_args.custom_client_headers.clone())
                        })
                        .map(Arc::new)
                        .map_err(|_| EdgeError::InvalidServerUrl(edge_args.upstream_url.clone()))
                        .expect("Could not construct UnleashClient");
                    Self {
                        unleash_client,
                        token: token.clone(),
                    }
                }))
            }
            _ => Ok(None),
        }
    }
}

pub async fn send_instance_data(
    instance_data_sender: Option<InstanceDataSender>,
    prometheus_registry: Registry,
    our_instance_data: Arc<EdgeInstanceData>,
    downstream_instance_data: Arc<RwLock<Vec<EdgeInstanceData>>>,
) {
    let mut do_the_work = true;
    loop {
        let mut empty = true;
        tokio::time::sleep(std::time::Duration::from_secs(15)).await;
        if let Some(instance_data_sender) = instance_data_sender.clone() {
            trace!("Looping instance data sending");
            let observed_data = our_instance_data.observe(
                &prometheus_registry,
                downstream_instance_data.read().await.clone(),
            );
            if do_the_work {
                let status = instance_data_sender
                    .unleash_client
                    .send_instance_data(observed_data, &instance_data_sender.token)
                    .await;
                match status {
                    Ok(_) => {}
                    Err(e) => match e {
                        EdgeError::EdgeMetricsRequestError(status, _message) => {
                            warn!("Failed to post instance data with status {status}");
                            if status == StatusCode::NOT_FOUND {
                                debug!("Upstream edge metrics not found, clearing our data about downstream instances to avoid growing to infinity (and beyond!).");
                                empty = true;
                                do_the_work = false;
                            } else if status == StatusCode::FORBIDDEN {
                                warn!("Upstream edge metrics rejected our data, clearing our data about downstream instances to avoid growing to infinity (and beyond!)");
                                empty = true;
                                do_the_work = false;
                            }
                        }
                        _ => {
                            warn!("Failed to post instance data due to unknown error {e:?}");
                            empty = false;
                        }
                    },
                }
            }
        } else {
            debug!("Did not have something to send instance data to");
            empty = true; // Emptying here, since we don't have anywhere to send the data to, to avoid growing memory
        }
        if empty {
            downstream_instance_data.write().await.clear();
        }
    }
}
