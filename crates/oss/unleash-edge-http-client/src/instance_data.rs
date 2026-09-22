use reqwest::{StatusCode, Url};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::{ClientMetaInformation, UnleashClient};
use tracing::{debug, info, warn};
use unleash_edge_cli::AuthHeaders;
use unleash_edge_types::errors::EdgeError;
use unleash_edge_types::metrics::instance_data::EdgeInstanceData;
use unleash_edge_types::{BackgroundTask, TokenCache, TokenType, TokenValidationStatus};

#[derive(Debug, Clone)]
pub struct InstanceDataSender {
    pub unleash_client: Arc<UnleashClient>,
    pub base_path: String,
}

#[derive(Debug, Clone)]
pub enum InstanceDataSending {
    SendNothing,
    SendInstanceData(InstanceDataSender),
}

impl InstanceDataSending {
    pub fn from_args(
        auth_headers: AuthHeaders,
        upstream_url: Url,
        client_meta_information: &ClientMetaInformation,
        custom_client_headers: Vec<(String, String)>,
        base_path: String,
        http_client: reqwest::Client,
    ) -> Result<Self, EdgeError> {
        let unleash_client = Arc::new(
            UnleashClient::from_url_with_backing_client(
                upstream_url,
                auth_headers
                    .upstream_auth_header
                    .clone()
                    .unwrap_or("Authorization".to_string()),
                http_client,
                client_meta_information.clone(),
            )
            .with_custom_client_headers(custom_client_headers.clone()),
        );
        let instance_data_sender = InstanceDataSender {
            unleash_client,
            base_path,
        };
        Ok(InstanceDataSending::SendInstanceData(instance_data_sender))
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
    token: &str,
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
                .post_edge_observability_data(observed_data, token)
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
    token_cache: Arc<TokenCache>,
) -> BackgroundTask {
    let instance_data_sender = instance_data_sender.clone();
    let our_instance_data = our_instance_data.clone();
    let downstream_instance_data = downstream_instance_data.clone();
    let our_cache = token_cache.clone();
    Box::pin(async move {
        if let Some(our_token) = find_first_valid_token(&our_cache) {
            let result = send_instance_data(
                &instance_data_sender,
                &our_instance_data,
                &downstream_instance_data,
                &our_token,
            )
            .await;
            if let Err(err) = result {
                warn!("Failed to send last set of instance data during graceful exit: {err:?}");
            }
        } else {
            info!("No valid token found in cache, not sending instance data during graceful exit");
        }
    })
}

pub fn create_send_instance_data_task(
    instance_data_sender: Arc<InstanceDataSending>,
    our_instance_data: Arc<EdgeInstanceData>,
    downstream_instance_data: Arc<RwLock<Vec<EdgeInstanceData>>>,
    token_cache: Arc<TokenCache>,
) -> BackgroundTask {
    let mut errors = 0;
    let delay = std::time::Duration::from_secs(60);
    Box::pin(async move {
        loop {
            tokio::time::sleep(
                std::time::Duration::from_secs(60) + delay * std::cmp::min(errors, 10),
            )
            .await;
            if let Some(token) = find_first_valid_token(&token_cache) {
                let result = send_instance_data(
                    &instance_data_sender,
                    &our_instance_data,
                    &downstream_instance_data,
                    &token,
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
            } else {
                debug!("No valid token found, skipping instance data send");
            }
        }
    })
}

fn find_first_valid_token(token_cache: &TokenCache) -> Option<String> {
    token_cache.iter().find_map(|t| {
        if t.status == TokenValidationStatus::Validated && t.token_type == Some(TokenType::Backend)
        {
            Some(t.value().token.clone())
        } else {
            None
        }
    })
}
