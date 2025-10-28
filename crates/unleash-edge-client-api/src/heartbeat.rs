use axum::extract::{FromRef, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use std::sync::Arc;
use tracing::{debug, info, instrument};
use ulid::Ulid;
use unleash_edge_appstate::AppState;
use unleash_edge_appstate::edge_token_extractor::{AuthState, AuthToken};
use unleash_edge_http_client::UnleashClient;
use unleash_edge_types::enterprise::{HeartbeatResponse, LicenseState};
use utoipa::IntoParams;

#[derive(Clone, Debug, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
struct ConnectionId {
    #[param(value_type = String, example = "01J5X6F2Q9H8K4R1S2T3U4V5W6")]
    #[serde(deserialize_with = "deserialize_ulid")]
    pub connection_id: Ulid,
}

fn deserialize_ulid<'de, D>(deserializer: D) -> Result<Ulid, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    Ulid::from_string(&s).map_err(serde::de::Error::custom)
}

#[utoipa::path(
    path = "/heartbeat",
    post,
    context_path = "/api/client/edge-licensing",
    responses(
        (status = 202, description = "Connection id was accepted and will be forwarded to Unleash Server soon", body = HeartbeatResponse),
        (status = 503, description = "Upstream Edge instance was unable to license itself and so cannot license downstream instances"),
    ),
    security(
        ("Authorization" = [])
    )
)]
#[instrument(skip(app_state, edge_token))]
async fn heartbeat(
    State(app_state): State<HeartbeatState>,
    AuthToken(edge_token): AuthToken,
    Query(query_params): Query<ConnectionId>,
) -> impl IntoResponse {
    tokio::spawn(async move {
        match app_state
            .client
            .send_heartbeat(&edge_token, &query_params.connection_id)
            .await
        {
            Err(e) => {
                info!("Unexpected error sending heartbeat: {}", e);
            }
            Ok(_) => {
                debug!("Successfully forwarded heartbeat for downstream instance");
            }
        };
    });

    (
        StatusCode::ACCEPTED,
        Json(HeartbeatResponse {
            edge_license_state: app_state.license_state,
        }),
    )
}

#[derive(Clone)]
pub struct HeartbeatState {
    license_state: LicenseState,
    client: Arc<UnleashClient>,
}

impl FromRef<AppState> for HeartbeatState {
    fn from_ref(app_state: &AppState) -> Self {
        let client = app_state
            .token_validator
            .as_ref()
            .as_ref()
            .map(|v| v.unleash_client.clone())
            .expect("Not running in Edge mode but enterprise Edge requires this for licensing");

        HeartbeatState {
            client,
            license_state: LicenseState::Valid, //TODO: get this from either the API call or Backup. This probably needs to be set at the AppState level
        }
    }
}

pub(crate) fn heartbeat_router_for<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
    HeartbeatState: FromRef<S>,
    AuthState: FromRef<S>,
{
    Router::new().route("/edge-licensing/heartbeat", post(heartbeat))
}

pub fn router() -> Router<AppState> {
    heartbeat_router_for::<AppState>()
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use axum_test::TestServer;
    use chrono::Duration;
    use reqwest::Url;
    use tokio::time::Duration as TokioDuration;
    use tokio::{sync::oneshot, time::timeout};
    use unleash_edge_cli::AuthHeaders;
    use unleash_edge_http_client::{ClientMetaInformation, HttpClientArgs, new_reqwest_client};
    use unleash_edge_types::{TokenCache, tokens::EdgeToken};

    use super::*;

    #[derive(Clone)]
    struct TestState {
        client: Arc<UnleashClient>,
        tokens: Vec<EdgeToken>,
    }

    impl FromRef<TestState> for HeartbeatState {
        fn from_ref(app: &TestState) -> Self {
            HeartbeatState {
                license_state: LicenseState::Valid,
                client: app.client.clone(),
            }
        }
    }

    impl FromRef<TestState> for AuthState {
        fn from_ref(state: &TestState) -> Self {
            let token_cache = Arc::new(TokenCache::default());
            for token in state.tokens.iter() {
                token_cache.insert(token.token.clone(), token.clone());
            }
            AuthState {
                auth_headers: AuthHeaders::default(),
                token_cache,
            }
        }
    }

    fn create_test_client(url: Url) -> UnleashClient {
        let client_meta_information = ClientMetaInformation {
            app_name: "unleash-edge-test".into(),
            instance_id: Ulid::new(),
            connection_id: Ulid::new(),
        };

        UnleashClient::from_url_with_backing_client(
            url,
            "Authorization".to_string(),
            new_reqwest_client(HttpClientArgs {
                skip_ssl_verification: false,
                client_identity: None,
                upstream_certificate_file: None,
                connect_timeout: Duration::seconds(5),
                socket_timeout: Duration::seconds(5),
                keep_alive_timeout: Duration::seconds(15),
                client_meta_information: client_meta_information.clone(),
            })
            .expect("Failed to create client"),
            client_meta_information,
        )
    }

    fn build_server(app_state: TestState) -> TestServer {
        let router = Router::new()
            .nest("/api/client", super::heartbeat_router_for::<TestState>())
            .with_state(app_state);

        TestServer::builder()
            .http_transport()
            .build(router)
            .unwrap()
    }

    #[tokio::test]
    async fn accepts_connection_id_and_forwards_heartbeat() {
        let token =
            EdgeToken::from_str("*:development.abc123def").expect("Failed to build edge token");

        // build the the upstream server, we're using this to ensure that our responses get forwarded correctly
        let (seen_tx, seen_rx) = oneshot::channel::<Ulid>();
        let seen_tx = std::sync::Arc::new(std::sync::Mutex::new(Some(seen_tx)));
        let seen_tx_clone = seen_tx.clone();

        let upstream = TestServer::builder()
            .http_transport()
            .build(Router::new().route(
                "/api/client/edge-licensing/heartbeat",
                post({
                    move |_: axum::http::HeaderMap, Query(conn): Query<ConnectionId>| {
                        if let Some(tx) = seen_tx_clone.lock().unwrap().take() {
                            let _ = tx.send(conn.connection_id);
                        }
                        async move {
                            (
                                StatusCode::ACCEPTED,
                                Json(HeartbeatResponse {
                                    edge_license_state: LicenseState::Valid,
                                }),
                            )
                        }
                    }
                }),
            ))
            .unwrap();

        // build the server that we're actually testing. We expect this to forward responses to upstream
        let app_state = TestState {
            client: Arc::new(create_test_client(upstream.server_url("/").unwrap())),
            tokens: vec![token.clone()],
        };
        let test_server = build_server(app_state);

        // poke our server a request - this should make it all the way to upstream
        let connection_id = Ulid::new();
        let response = test_server
            .post("/api/client/edge-licensing/heartbeat")
            .add_query_param("connectionId", connection_id.to_string())
            .add_header("Authorization", format!("{}", token.token))
            .await;

        let seen_ulid = timeout(TokioDuration::from_millis(200), seen_rx)
            .await
            .expect("upstream handler not called in time")
            .expect("upstream sender dropped unexpectedly");

        response.assert_status(StatusCode::ACCEPTED);
        assert_eq!(seen_ulid, connection_id);
    }

    #[tokio::test]
    async fn missing_upstream_still_sends_back_valid_response_if_edge_has_license_state() {
        let token =
            EdgeToken::from_str("*:development.abc123def").expect("Failed to build edge token");

        let app_state = TestState {
            client: Arc::new(create_test_client(
                Url::parse("http://invalid-edge-upstream.invalid").unwrap(),
            )),
            tokens: vec![token.clone()],
        };
        let test_server = build_server(app_state);

        let connection_id = Ulid::new();
        let response = test_server
            .post("/api/client/edge-licensing/heartbeat")
            .add_query_param("connectionId", connection_id.to_string())
            .add_header("Authorization", format!("{}", token.token))
            .await;

        response.assert_status(StatusCode::ACCEPTED);
        response.assert_json(&HeartbeatResponse {
            edge_license_state: LicenseState::Valid,
        });
    }
}
