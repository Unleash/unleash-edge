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
    NoBackoff(String),
    Backoff(String),
}

async fn clear_reported_data(
    our_instance_data: &Arc<EdgeInstanceData>,
    downstream_instance_data: &Arc<RwLock<Vec<EdgeInstanceData>>>,
) {
    downstream_instance_data.write().await.clear();
    our_instance_data.clear_time_windowed_metrics();
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
                                clear_reported_data(our_instance_data, downstream_instance_data)
                                    .await;
                                Err(InstanceDataSendError::Backoff("Our upstream is not running a version that supports edge metrics.".into()))
                            }
                            StatusCode::FORBIDDEN => {
                                clear_reported_data(our_instance_data, downstream_instance_data)
                                    .await;
                                Err(InstanceDataSendError::Backoff("Upstream edge metrics said our token wasn't allowed to post data".into()))
                            }

                            StatusCode::PAYLOAD_TOO_LARGE | StatusCode::BAD_REQUEST => {
                                // drop the buffer - upstream will never take a body this big
                                // so the next is small enough to get through
                                let dropped = downstream_instance_data.read().await.len();
                                clear_reported_data(our_instance_data, downstream_instance_data)
                                    .await;
                                Err(InstanceDataSendError::NoBackoff(format!(
                                    "Upstream rejected our instance data with {status}. Dropped {dropped} downstream instance(s) so the next report fits."
                                )))
                            }
                            _ => Err(InstanceDataSendError::Backoff(format!(
                                "Failed to post instance data: {e:?}"
                            ))),
                        }
                    }

                    _ => Err(InstanceDataSendError::Backoff(format!(
                        "Failed to post instance data: {e:?}"
                    ))),
                }
            } else {
                clear_reported_data(our_instance_data, downstream_instance_data).await;
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
                    }
                    Err(InstanceDataSendError::NoBackoff(message)) => {
                        warn!(message);
                    }
                    Err(InstanceDataSendError::Backoff(message)) => {
                        warn!(message);
                        errors += 1;
                    }
                }
            } else {
                debug!("No valid token found, skipping instance data send");
            }
        }
    })
}

fn find_first_valid_token(token_cache: &TokenCache) -> Option<String> {
    token_cache
        .iter()
        .find_map(|t| match (&t.status, &t.token_type) {
            (TokenValidationStatus::Validated, Some(TokenType::Backend)) => Some(t.token.clone()),
            _ => None,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::assert_matches;
    use unleash_edge_types::tokens::EdgeToken;

    #[test]
    pub fn find_first_valid_token_skips_any_token_not_validated() {
        let invalid = EdgeToken::offline_token("mytoken");
        let token_cache = TokenCache::default();
        token_cache.insert(invalid.token.clone(), invalid);
        assert_matches!(find_first_valid_token(&token_cache), None);
    }
    #[test]
    pub fn skips_frontend_tokens_and_picks_backend_tokens() {
        let admin = EdgeToken::admin_token("*:*.abc132");
        let mut frontend = EdgeToken::from_trimmed_str("*:production.abc123")
            .expect("Failed to parse frontend token");
        frontend.status = TokenValidationStatus::Validated;
        frontend.token_type = Some(TokenType::Frontend);
        let mut backend = EdgeToken::from_trimmed_str("*:production.123abc")
            .expect("Failed to parse backend token");
        backend.status = TokenValidationStatus::Validated;
        backend.token_type = Some(TokenType::Backend);
        let token_cache = TokenCache::default();
        token_cache.insert(frontend.token.clone(), frontend);
        token_cache.insert(admin.token.clone(), admin);
        token_cache.insert(backend.token.clone(), backend.clone());
        let found = find_first_valid_token(&token_cache);
        assert_eq!(found, Some(backend.token));
    }

    fn upstream_answering(status: StatusCode) -> axum_test::TestServer {
        let router = axum::Router::new().route(
            "/api/client/metrics/edge",
            axum::routing::post(move || async move { status }),
        );
        axum_test::TestServer::builder()
            .http_transport()
            .build(router)
    }

    fn sender_posting_to(upstream: Url) -> Arc<InstanceDataSending> {
        let http_client = crate::new_reqwest_client(crate::HttpClientArgs {
            skip_ssl_verification: false,
            client_identity: None,
            upstream_certificate_file: None,
            connect_timeout: chrono::Duration::seconds(5),
            socket_timeout: chrono::Duration::seconds(5),
            keep_alive_timeout: chrono::Duration::seconds(5),
            client_meta_information: ClientMetaInformation::default(),
        })
        .expect("Failed to build backing client");
        Arc::new(
            InstanceDataSending::from_args(
                AuthHeaders::default(),
                upstream,
                &ClientMetaInformation::default(),
                vec![],
                "".into(),
                http_client,
            )
            .expect("Failed to build instance data sender"),
        )
    }

    /// One downstream Edge has reported in since our last successful post.
    fn buffer_holding_one_report() -> Arc<RwLock<Vec<EdgeInstanceData>>> {
        Arc::new(RwLock::new(vec![EdgeInstanceData::new(
            "downstream-edge",
            &ulid::Ulid::generate(),
            None,
        )]))
    }

    async fn post_to_upstream_answering(
        status: StatusCode,
        downstream: &Arc<RwLock<Vec<EdgeInstanceData>>>,
    ) -> Result<(), InstanceDataSendError> {
        let upstream = upstream_answering(status);
        let sender = sender_posting_to(upstream.server_url("/").expect("Failed to build url"));
        let ours = Arc::new(EdgeInstanceData::new(
            "our-edge",
            &ulid::Ulid::generate(),
            None,
        ));
        send_instance_data(&sender, &ours, downstream, "some-backend-token").await
    }

    #[tokio::test]
    async fn too_large_a_payload_is_dropped_so_the_next_one_can_get_through() {
        let downstream = buffer_holding_one_report();
        let result = post_to_upstream_answering(StatusCode::PAYLOAD_TOO_LARGE, &downstream).await;
        assert!(
            matches!(result, Err(InstanceDataSendError::NoBackoff(_))),
            "a dropped payload should not stretch the send interval, got {result:?}"
        );
        assert!(
            downstream.read().await.is_empty(),
            "the buffer that made the payload too large should have been dropped"
        );
    }

    #[tokio::test]
    async fn a_struggling_upstream_does_not_cost_us_the_buffered_reports() {
        let downstream = buffer_holding_one_report();
        let result =
            post_to_upstream_answering(StatusCode::INTERNAL_SERVER_ERROR, &downstream).await;
        assert!(
            matches!(result, Err(InstanceDataSendError::Backoff(_))),
            "a struggling upstream should make us back off, got {result:?}"
        );
        assert_eq!(
            downstream.read().await.len(),
            1,
            "the report is still good, so it should survive for the next attempt"
        );
    }

    #[tokio::test]
    async fn forbidden_and_not_found_keep_clearing_and_backing_off() {
        for status in [StatusCode::FORBIDDEN, StatusCode::NOT_FOUND] {
            let downstream = buffer_holding_one_report();
            let result = post_to_upstream_answering(status, &downstream).await;
            assert!(
                matches!(result, Err(InstanceDataSendError::Backoff(_))),
                "{status} should back off, got {result:?}"
            );
            assert!(
                downstream.read().await.is_empty(),
                "{status} should clear the buffer, as it always has"
            );
        }
    }

    #[tokio::test]
    async fn a_successful_post_closes_the_window() {
        let downstream = buffer_holding_one_report();
        let result = post_to_upstream_answering(StatusCode::ACCEPTED, &downstream).await;
        assert!(result.is_ok(), "expected a successful post, got {result:?}");
        assert!(downstream.read().await.is_empty());
    }
}
