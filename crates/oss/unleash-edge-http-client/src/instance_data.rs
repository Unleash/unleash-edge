use prometheus::{Histogram, IntGauge, register_histogram, register_int_gauge};
use reqwest::{StatusCode, Url};
use std::sync::{Arc, LazyLock};
use tokio::sync::RwLock;

use crate::{ClientMetaInformation, UnleashClient};
use tracing::{debug, info, warn};
use unleash_edge_cli::AuthHeaders;
use unleash_edge_types::errors::EdgeError;
use unleash_edge_types::metrics::instance_data::{EdgeInstanceData, merge_reports, report_counts};
use unleash_edge_types::metrics::payload_size::{
    TARGET_BODY_SIZE, UPSTREAM_MAX_BODY_SIZE, serialized_len,
};
use unleash_edge_types::{BackgroundTask, TokenCache, TokenType, TokenValidationStatus};

const PAYLOAD_SIZE_BUCKETS: [f64; 8] = [
    8.0 * 1024.0,
    20.0 * 1024.0,
    50.0 * 1024.0,
    64.0 * 1024.0,
    TARGET_BODY_SIZE as f64,
    UPSTREAM_MAX_BODY_SIZE as f64,
    256.0 * 1024.0,
    1024.0 * 1024.0,
];

static PAYLOAD_BYTES: LazyLock<Histogram> = LazyLock::new(|| {
    register_histogram!(
        "edge_instance_data_payload_bytes",
        "Serialized size of the instance data payload posted upstream",
        PAYLOAD_SIZE_BUCKETS.to_vec()
    )
    .expect("failed to register edge_instance_data_payload_bytes")
});

static ROOT_PAYLOAD_BYTES: LazyLock<Histogram> = LazyLock::new(|| {
    register_histogram!(
        "edge_instance_data_root_payload_bytes",
        "Serialized size of our own instance data, excluding downstream reports",
        PAYLOAD_SIZE_BUCKETS.to_vec()
    )
    .expect("failed to register edge_instance_data_root_payload_bytes")
});

static DOWNSTREAM_REPORTS: LazyLock<IntGauge> = LazyLock::new(|| {
    register_int_gauge!(
        "edge_instance_data_downstream_reports",
        "Downstream instance reports in the payload we last sent, counted through the whole tree"
    )
    .expect("failed to register edge_instance_data_downstream_reports")
});

static DOWNSTREAM_DISTINCT_REPORTS: LazyLock<IntGauge> = LazyLock::new(|| {
    register_int_gauge!(
        "edge_instance_data_distinct_downstream_reports",
        "How many of those came from a distinct instance. Well below the total means the same \
        instances reported several times over, which is what grows this payload past the budget"
    )
    .expect("failed to register edge_instance_data_distinct_downstream_reports")
});

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

/// Hand the reports back after a failure we will retry. Reports that arrived while the post was
/// in flight are folded on top, so they count as the newer ones.
async fn retain_reports(
    downstream_instance_data: &Arc<RwLock<Vec<EdgeInstanceData>>>,
    mut reports: Vec<EdgeInstanceData>,
) {
    let mut buffer = downstream_instance_data.write().await;
    merge_reports(&mut reports, std::mem::take(&mut buffer));
    *buffer = reports;
}

struct PayloadSize {
    total: usize,
    root: usize,
    reports: usize,
    distinct: usize,
}

fn measure_payload(mut payload: EdgeInstanceData) -> (EdgeInstanceData, PayloadSize) {
    let downstream = std::mem::take(&mut payload.connected_edges);

    let (reports, distinct) = report_counts(&downstream);
    DOWNSTREAM_REPORTS.set(reports as i64);
    DOWNSTREAM_DISTINCT_REPORTS.set(distinct as i64);

    let root = serialized_len(&payload);
    ROOT_PAYLOAD_BYTES.observe(root as f64);

    payload.connected_edges = downstream;
    let total = serialized_len(&payload);
    PAYLOAD_BYTES.observe(total as f64);

    let size = PayloadSize {
        total,
        root,
        reports,
        distinct,
    };
    (payload, size)
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
            // Take the buffer rather than copy it: a report that arrives while this post is
            // in flight then lands in a fresh buffer instead of being cleared along with the
            // ones we sent. The failures we mean to retry hand these back.
            let reports = std::mem::take(&mut *downstream_instance_data.write().await);
            let observed_data =
                our_instance_data.observe(reports.clone(), &instance_data_sender.base_path);
            let (observed_data, size) = measure_payload(observed_data);
            let status = instance_data_sender
                .unleash_client
                .post_edge_observability_data(observed_data, token)
                .await;

            if let Err(e) = status {
                match e {
                    EdgeError::EdgeMetricsRequestError(status, _) => {
                        match status {
                            StatusCode::NOT_FOUND => {
                                our_instance_data.clear_time_windowed_metrics();
                                Err(InstanceDataSendError::Backoff("Our upstream is not running a version that supports edge metrics.".into()))
                            }
                            StatusCode::FORBIDDEN => {
                                our_instance_data.clear_time_windowed_metrics();
                                Err(InstanceDataSendError::Backoff("Upstream edge metrics said our token wasn't allowed to post data".into()))
                            }

                            StatusCode::PAYLOAD_TOO_LARGE | StatusCode::BAD_REQUEST => {
                                // drop the buffer - upstream will never take a body this big
                                // so the next is small enough to get through
                                our_instance_data.clear_time_windowed_metrics();
                                Err(InstanceDataSendError::NoBackoff(format!(
                                    "Upstream rejected our instance data with {status}. It was \
                                     {} bytes against a {UPSTREAM_MAX_BODY_SIZE} byte limit, \
                                     {} of it our own instance, carrying {} report(s) from {} \
                                     distinct downstream instance(s). Dropped the buffered \
                                     reports so the next payload fits.",
                                    size.total, size.root, size.reports, size.distinct
                                )))
                            }
                            _ => {
                                retain_reports(downstream_instance_data, reports).await;
                                Err(InstanceDataSendError::Backoff(format!(
                                    "Failed to post instance data: {e:?}"
                                )))
                            }
                        }
                    }

                    _ => {
                        retain_reports(downstream_instance_data, reports).await;
                        Err(InstanceDataSendError::Backoff(format!(
                            "Failed to post instance data: {e:?}"
                        )))
                    }
                }
            } else {
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
const MAX_BACKOFF_STEPS: u32 = 8;

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
                std::time::Duration::from_secs(60)
                    + delay * std::cmp::min(errors, MAX_BACKOFF_STEPS),
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

    /// one downstream Edge has reported in since our last successful post
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
        post_to(&upstream_answering(status), downstream).await
    }

    async fn post_to(
        upstream: &axum_test::TestServer,
        downstream: &Arc<RwLock<Vec<EdgeInstanceData>>>,
    ) -> Result<(), InstanceDataSendError> {
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

    /// An upstream that answers `status`, after a report from `identifier` reaches our buffer
    /// while it is handling the post.
    fn upstream_racing_a_report(
        status: StatusCode,
        downstream: &Arc<RwLock<Vec<EdgeInstanceData>>>,
        identifier: ulid::Ulid,
    ) -> axum_test::TestServer {
        let downstream = downstream.clone();
        let router = axum::Router::new().route(
            "/api/client/metrics/edge",
            axum::routing::post(move || async move {
                let late = EdgeInstanceData::new("downstream-edge", &identifier, None);
                late.observe_request_consumption();
                downstream.write().await.push(late);
                status
            }),
        );
        axum_test::TestServer::builder()
            .http_transport()
            .build(router)
    }

    #[tokio::test]
    async fn a_report_arriving_during_a_successful_post_waits_for_the_next_one() {
        let downstream = buffer_holding_one_report();
        let upstream =
            upstream_racing_a_report(StatusCode::ACCEPTED, &downstream, ulid::Ulid::generate());

        assert!(post_to(&upstream, &downstream).await.is_ok());

        assert_eq!(
            downstream.read().await.len(),
            1,
            "only the report we sent is cleared, not one that came in while we sent it"
        );
    }

    #[tokio::test]
    async fn a_report_arriving_during_a_failed_post_folds_into_the_retained_one() {
        let identifier = ulid::Ulid::generate();
        let held = EdgeInstanceData::new("downstream-edge", &identifier, None);
        held.observe_request_consumption();
        let downstream = Arc::new(RwLock::new(vec![held]));
        let upstream =
            upstream_racing_a_report(StatusCode::INTERNAL_SERVER_ERROR, &downstream, identifier);

        assert!(post_to(&upstream, &downstream).await.is_err());

        let buffer = downstream.read().await;
        assert_eq!(buffer.len(), 1, "two windows from one instance, one entry");
        assert_eq!(
            buffer[0]
                .request_consumption_since_last_report
                .get_requests("default"),
            2
        );
    }

    #[test]
    fn measuring_a_payload_does_not_change_it() {
        let mut payload = EdgeInstanceData::new("our-edge", &ulid::Ulid::generate(), None);
        payload.connected_edges = vec![
            EdgeInstanceData::new("downstream-one", &ulid::Ulid::generate(), None),
            EdgeInstanceData::new("downstream-two", &ulid::Ulid::generate(), None),
        ];
        let before = serialized_len(&payload);

        let (payload, size) = measure_payload(payload);

        assert_eq!(payload.connected_edges.len(), 2);
        assert_eq!(
            serialized_len(&payload),
            before,
            "measuring a payload must leave it byte for byte the same"
        );
        assert_eq!(size.total, before);
        assert!(
            size.root < size.total,
            "our own instance cannot be as big as the payload carrying two reports"
        );
    }

    #[test]
    fn reports_are_counted_through_the_whole_tree() {
        let mut grandchild = EdgeInstanceData::new("tier-three", &ulid::Ulid::generate(), None);
        grandchild.connected_edges = vec![EdgeInstanceData::new(
            "tier-four",
            &ulid::Ulid::generate(),
            None,
        )];
        let mut child = EdgeInstanceData::new("tier-two", &ulid::Ulid::generate(), None);
        child.connected_edges = vec![grandchild];
        let mut payload = EdgeInstanceData::new("tier-one", &ulid::Ulid::generate(), None);
        payload.connected_edges = vec![child];

        let (_, size) = measure_payload(payload);

        assert_eq!(
            size.reports, 3,
            "a three tier chain below us is three reports, not one"
        );
        assert_eq!(size.distinct, 3);
    }

    #[test]
    fn repeated_reports_from_one_instance_count_once_as_distinct() {
        let identifier = ulid::Ulid::generate();
        let twice = vec![
            EdgeInstanceData::new("downstream", &identifier, None),
            EdgeInstanceData::new("downstream", &identifier, None),
        ];

        assert_eq!(report_counts(&twice), (2, 1));
    }

    #[test]
    fn payload_size_buckets_ascend() {
        assert!(
            PAYLOAD_SIZE_BUCKETS
                .windows(2)
                .all(|pair| pair[0] < pair[1]),
            "bucket bounds must ascend: {PAYLOAD_SIZE_BUCKETS:?}"
        );
    }
}
