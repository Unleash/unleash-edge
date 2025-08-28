use reqwest::{StatusCode, Url};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::{ClientMetaInformation, UnleashClient};
use tracing::{debug, warn};
use unleash_edge_cli::{CliArgs, EdgeMode};
use unleash_edge_types::BackgroundTask;
use unleash_edge_types::errors::EdgeError;
use unleash_edge_types::metrics::instance_data::EdgeInstanceData;

#[derive(Debug, Clone)]
pub struct InstanceDataSender {
    pub unleash_client: Arc<UnleashClient>,
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
                    };
                    InstanceDataSending::SendInstanceData(instance_data_sender)
                })
                .map(Ok)
                .unwrap_or(Ok(InstanceDataSending::SendNothing)),
            _ => Ok(InstanceDataSending::SendNothing),
        }
    }
}

#[derive(Debug)]
enum InstanceDataSendError {
    Backoff(String),
    Unexpected(String),
}

async fn send_instance_data(
    instance_data_sender: &Arc<InstanceDataSending>,
    our_instance_data: &Arc<EdgeInstanceData>,
    downstream_instance_data: &Arc<RwLock<Vec<EdgeInstanceData>>>,
) -> Result<(), InstanceDataSendError> {
    match instance_data_sender.as_ref() {
        InstanceDataSending::SendNothing => {
            debug!("No instance data sender found. Doing nothing.");
            Ok(())
        }
        InstanceDataSending::SendInstanceData(instance_data_sender) => {
            let observed_data = our_instance_data.observe(
                downstream_instance_data.read().await.clone(),
                &instance_data_sender.base_path,
            );
            let status = instance_data_sender
                .unleash_client
                .post_edge_observability_data(observed_data, &instance_data_sender.token)
                .await;

            if let Err(e) = status {
                match e {
                    EdgeError::EdgeMetricsRequestError(status, _) => {
                        match status {
                            StatusCode::NOT_FOUND => {
                                downstream_instance_data.write().await.clear();
                                our_instance_data.clear_time_windowed_metrics();
                                Err(InstanceDataSendError::Backoff("Our upstream is not running a version that supports edge metrics.".into()))
                            }
                            StatusCode::FORBIDDEN => {
                                downstream_instance_data.write().await.clear();
                                our_instance_data.clear_time_windowed_metrics();
                                Err(InstanceDataSendError::Backoff("Upstream edge metrics said our token wasn't allowed to post data".into()))
                            }
                            _ => Err(InstanceDataSendError::Unexpected(format!(
                                "Failed to post instance data due to unknown error {e:?}"
                            ))),
                        }
                    }
                    _ => Err(InstanceDataSendError::Unexpected(format!(
                        "Failed to post instance data due to unknown error {e:?}"
                    ))),
                }
            } else {
                downstream_instance_data.write().await.clear();
                our_instance_data.clear_time_windowed_metrics();
                Ok(())
            }
        }
    }
}

pub fn create_once_off_send_instance_data(
    instance_data_sender: Arc<InstanceDataSending>,
    our_instance_data: Arc<EdgeInstanceData>,
    downstream_instance_data: Arc<RwLock<Vec<EdgeInstanceData>>>,
) -> BackgroundTask {
    let instance_data_sender = instance_data_sender.clone();
    let our_instance_data = our_instance_data.clone();
    let downstream_instance_data = downstream_instance_data.clone();

    Box::pin(async move {
        let result = send_instance_data(
            &instance_data_sender,
            &our_instance_data,
            &downstream_instance_data,
        )
        .await;

        if let Err(err) = result {
            warn!("Failed to send last set of instance data during graceful exit: {err:?}");
        }
    })
}

pub fn create_send_instance_data_task(
    instance_data_sender: Arc<InstanceDataSending>,
    our_instance_data: Arc<EdgeInstanceData>,
    downstream_instance_data: Arc<RwLock<Vec<EdgeInstanceData>>>,
) -> BackgroundTask {
    let mut errors = 0;
    let delay = std::time::Duration::from_secs(60);
    Box::pin(async move {
        loop {
            tokio::time::sleep(
                std::time::Duration::from_secs(60) + delay * std::cmp::min(errors, 10),
            )
            .await;

            let result = send_instance_data(
                &instance_data_sender,
                &our_instance_data,
                &downstream_instance_data,
            )
            .await;
            match result {
                Ok(_) => {
                    debug!("Successfully posted observability metrics.");
                    errors = 0;
                    downstream_instance_data.write().await.clear();
                    our_instance_data.clear_time_windowed_metrics();
                }
                Err(err) => match err {
                    InstanceDataSendError::Backoff(message) => {
                        warn!(message);
                        errors += 1;
                    }
                    InstanceDataSendError::Unexpected(message) => {
                        warn!(message);
                    }
                },
            }
        }
    })
}
