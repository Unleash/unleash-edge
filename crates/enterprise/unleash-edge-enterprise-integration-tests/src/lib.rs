#[cfg(test)]
mod tests {
    use ahash::HashMap;
    use async_trait::async_trait;
    use axum::Json;
    use axum::extract::State;
    use axum::http::StatusCode;
    use axum::routing::post;
    use axum::{Router, response::IntoResponse};
    use axum_test::TestServer;
    use chrono::Duration;
    use reqwest::{Client, Url};
    use serde_json::json;
    use std::str::FromStr;
    use std::sync::Arc;
    use tokio::sync::RwLock;
    use ulid::Ulid;
    use unleash_edge::edge_builder::{
        EdgeStateArgs, PersistenceArgs, build_edge_state, resolve_license,
    };
    use unleash_edge_cli::OtelExporterProtocol::Grpc;
    use unleash_edge_cli::{AuthHeaders, EdgeArgs, LogFormat};
    use unleash_edge_http_client::{
        ClientMetaInformation, HttpClientArgs, UnleashClient, new_reqwest_client,
    };
    use unleash_edge_persistence::EdgePersistence;
    use unleash_edge_types::EdgeResult;
    use unleash_edge_types::enterprise::LicenseState;
    use unleash_edge_types::errors::EdgeError;
    use unleash_edge_types::metrics::instance_data::{EdgeInstanceData, Hosting};
    use unleash_edge_types::tokens::EdgeToken;
    use unleash_types::client_features::ClientFeatures;

    #[derive(Clone)]
    struct MockState {
        license_result: EdgeResult<LicenseState>,
    }

    pub struct UpstreamMock {
        server: TestServer,
    }

    impl UpstreamMock {
        pub async fn new(initial: EdgeResult<LicenseState>) -> Self {
            let state = MockState {
                license_result: initial,
            };

            let app = Router::new()
                .nest(
                    "/api/client",
                    Router::new()
                        .route("/edge-licensing/heartbeat", post(Self::heartbeat))
                        .with_state(state.clone()),
                )
                .nest(
                    "/edge",
                    Router::new().route("/validate", post(Self::validate_all_tokens)),
                );

            let server = TestServer::builder().http_transport().build(app);

            Self { server }
        }

        pub fn url(&self) -> String {
            self.server.server_url("/").unwrap().to_string()
        }

        pub fn as_url(&self) -> Url {
            self.server.server_url("/").unwrap()
        }

        async fn heartbeat(State(s): State<MockState>) -> impl IntoResponse {
            match s.license_result {
                Ok(license_state) => (
                    StatusCode::ACCEPTED,
                    Json(json!({
                        "edgeLicenseState": license_state
                    })),
                ),
                Err(_) => (
                    StatusCode::FORBIDDEN,
                    Json(json!({"message": "License verification failed"})),
                ),
            }
        }

        async fn validate_all_tokens() -> impl IntoResponse {
            Json(json!({
                "tokens": [
                    {
                        "token": "*:development.hashyhashhash",
                        "type": "client",
                        "projects": ["*"]
                    }
                ]
            }))
        }
    }

    fn build_client(client_meta_information: &ClientMetaInformation) -> Client {
        new_reqwest_client(HttpClientArgs {
            skip_ssl_verification: false,
            client_identity: None,
            upstream_certificate_file: None,
            connect_timeout: Duration::seconds(10),
            socket_timeout: Duration::seconds(10),
            keep_alive_timeout: Duration::seconds(10),
            client_meta_information: client_meta_information.clone(),
        })
        .unwrap()
    }

    fn build_edge_state_data() -> (
        Client,
        ClientMetaInformation,
        Arc<RwLock<Vec<EdgeInstanceData>>>,
    ) {
        let client_meta_information = ClientMetaInformation {
            app_name: "unleash-edge-test".to_string(),
            connection_id: Ulid::new(),
            instance_id: Ulid::new(),
        };

        let http_client = build_client(&client_meta_information);
        let instances_observed_for_app_context = Arc::new(RwLock::new(vec![]));

        (
            http_client,
            client_meta_information,
            instances_observed_for_app_context,
        )
    }

    struct MockPersistence {
        license_state: EdgeResult<LicenseState>,
    }

    #[async_trait]
    impl EdgePersistence for MockPersistence {
        async fn load_license_state(&self) -> EdgeResult<LicenseState> {
            self.license_state.clone()
        }

        async fn save_license_state(&self, _license: &LicenseState) -> EdgeResult<()> {
            unimplemented!()
        }

        async fn load_tokens(&self) -> EdgeResult<Vec<EdgeToken>> {
            unimplemented!()
        }

        async fn load_features(&self) -> EdgeResult<HashMap<String, ClientFeatures>> {
            unimplemented!()
        }

        async fn save_tokens(&self, _tokens: Vec<EdgeToken>) -> EdgeResult<()> {
            unimplemented!()
        }

        async fn save_features(&self, _features: Vec<(String, ClientFeatures)>) -> EdgeResult<()> {
            unimplemented!()
        }
    }

    #[tokio::test]
    async fn enterprise_edge_state_errors_with_heartbeat_error_when_license_cannot_be_verified() {
        let upstream = UpstreamMock::new(Err(EdgeError::Forbidden("This Edge instance is not licensed! The police will arrive at your doorstep imminently".into()))).await;
        let (http_client, client_meta_information, instances_observed_for_app_context) =
            build_edge_state_data();

        let edge_args = EdgeArgs {
            upstream_url: upstream.url(),
            tokens: vec![EdgeToken::from_str("*:development.hashyhashhash").unwrap()],
            ..EdgeArgs::default()
        };

        let maybe_edge_state = build_edge_state(EdgeStateArgs {
            client_meta_information,
            instances_observed_for_app_context,
            auth_headers: AuthHeaders::default(),
            http_client,
            hosting_type: Hosting::SelfHosted,
            client_id: "".to_string(),
            app_id: Default::default(),
            otel_endpoint_url: None,
            otel_protocol: Grpc,
            log_format: LogFormat::Plain,
            upstream_url: upstream.as_url(),
            custom_client_headers: vec![],
            tokens: edge_args.tokens.clone(),
            base_path: "".to_string(),
            http_deny_list: None,
            http_allow_list: None,
            streaming: false,
            delta: false,
            persistence_args: PersistenceArgs::from(&edge_args),
            pretrusted_tokens: None,
            features_refresh_interval: Duration::seconds(30),
            metrics_interval_seconds: 30,
            token_revalidation_interval_seconds: 30,
            prometheus_remote_write_url: None,
            prometheus_push_interval: 0,
            prometheus_username: None,
            prometheus_password: None,
            hostname: None,
            ec2_instance_id: None,
        })
        .await;

        assert!(matches!(
            maybe_edge_state,
            Err(EdgeError::HeartbeatError(_, _))
        ));
    }

    #[tokio::test]
    async fn enterprise_edge_state_startup_succeeds_if_license_can_be_verified() {
        let upstream = UpstreamMock::new(Ok(LicenseState::Valid)).await;
        let (http_client, client_meta_information, instances_observed_for_app_context) =
            build_edge_state_data();

        let edge_args = EdgeArgs {
            upstream_url: upstream.url(),
            tokens: vec![EdgeToken::from_str("*:development.hashyhashhash").unwrap()],
            ..EdgeArgs::default()
        };

        let maybe_edge_state = build_edge_state(EdgeStateArgs {
            client_meta_information,
            instances_observed_for_app_context,
            auth_headers: AuthHeaders::default(),
            http_client,
            hosting_type: Hosting::SelfHosted,
            client_id: "".to_string(),
            app_id: Default::default(),
            otel_endpoint_url: None,
            otel_protocol: Grpc,
            log_format: LogFormat::Plain,
            upstream_url: upstream.as_url(),
            custom_client_headers: vec![],
            tokens: edge_args.tokens.clone(),
            base_path: "".to_string(),
            http_deny_list: None,
            http_allow_list: None,
            streaming: false,
            delta: false,
            persistence_args: Default::default(),
            pretrusted_tokens: None,
            features_refresh_interval: Default::default(),
            metrics_interval_seconds: 30i64,
            token_revalidation_interval_seconds: 30,
            prometheus_remote_write_url: None,
            prometheus_push_interval: 0,
            prometheus_username: None,
            prometheus_password: None,
            hostname: None,
            ec2_instance_id: None,
        })
        .await;

        assert!(maybe_edge_state.is_ok());
    }

    #[tokio::test]
    async fn resolving_a_license_falls_back_to_persistence_when_upstream_is_unreachable() {
        let client_meta_information = ClientMetaInformation {
            app_name: "unleash-edge-test".to_string(),
            connection_id: Ulid::new(),
            instance_id: Ulid::new(),
        };

        let unleash_client = UnleashClient::from_url_with_backing_client(
            Url::parse(
                "http://this-will-fail-dns-lookup-because-rfc2606-specifies-this-url-as.invalid",
            )
            .unwrap(),
            "Authorization".to_string(),
            build_client(&client_meta_information),
            client_meta_information.clone(),
        );

        let startup_tokens = vec![EdgeToken::from_str("*:development.hashyhashhash").unwrap()];

        let persistence: Option<Arc<dyn EdgePersistence + 'static>> =
            Some(Arc::new(MockPersistence {
                license_state: Ok(LicenseState::Valid),
            }));

        let sut = resolve_license(
            &unleash_client,
            persistence,
            &startup_tokens,
            &client_meta_information,
        );

        sut.await.expect("Expected license to be resolved");
    }

    #[tokio::test]
    async fn resolving_a_license_fails_when_upstream_is_unreachable_and_no_persistence() {
        let client_meta_information = ClientMetaInformation {
            app_name: "unleash-edge-test".to_string(),
            connection_id: Ulid::new(),
            instance_id: Ulid::new(),
        };

        let unleash_client = UnleashClient::from_url_with_backing_client(
            Url::parse(
                "http://this-will-fail-dns-lookup-because-rfc2606-specifies-this-url-as.invalid",
            )
            .unwrap(),
            "Authorization".to_string(),
            build_client(&client_meta_information),
            client_meta_information.clone(),
        );

        let startup_tokens = vec![EdgeToken::from_str("*:development.hashyhashhash").unwrap()];

        let persistence: Option<Arc<dyn EdgePersistence + 'static>> =
            Some(Arc::new(MockPersistence {
                license_state: Err(EdgeError::PersistenceError(
                    "You shouldn't have stored your data on a potato".into(),
                )),
            }));

        let sut = resolve_license(
            &unleash_client,
            persistence,
            &startup_tokens,
            &client_meta_information,
        );

        sut.await.expect_err("Expected license resolution to fail");
    }
}
