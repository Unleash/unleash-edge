#[cfg(test)]
mod tests {
    use ahash::HashMap;
    use async_trait::async_trait;
    use axum::Json;
    use axum::extract::State;
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
    use unleash_edge::edge_builder::{EdgeStateArgs, build_edge_state, resolve_license};
    use unleash_edge_cli::{AuthHeaders, CliArgs, EdgeArgs, HttpServerArgs};
    use unleash_edge_http_client::{
        ClientMetaInformation, HttpClientArgs, UnleashClient, new_reqwest_client,
    };
    use unleash_edge_persistence::EdgePersistence;
    use unleash_edge_types::enterprise::LicenseState;
    use unleash_edge_types::errors::EdgeError;
    use unleash_edge_types::metrics::instance_data::{EdgeInstanceData, Hosting};
    use unleash_edge_types::tokens::EdgeToken;
    use unleash_types::client_features::ClientFeatures;

    use unleash_edge_types::EdgeResult;

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

            let server = TestServer::builder()
                .http_transport()
                .build(app)
                .expect("failed to build upstream mock");

            Self { server }
        }

        pub fn url(&self) -> String {
            self.server.server_url("/").unwrap().to_string()
        }

        async fn heartbeat(State(s): State<MockState>) -> impl IntoResponse {
            if s.license_result.is_err() {
                return (
                    axum::http::StatusCode::FORBIDDEN,
                    Json(json!({"message": "License verification failed"})),
                );
            }

            return (
                axum::http::StatusCode::ACCEPTED,
                Json(json!({
                    "edgeLicenseState": match s.license_result {
                        Ok(license_state) => license_state,
                        Err(_) => LicenseState::Invalid,
                    }
                })),
            );
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

    fn mock_cli_args() -> CliArgs {
        CliArgs {
            http: HttpServerArgs {
                port: 3063,
                interface: "127.0.0.1".to_string(),
                base_path: "".to_string(),
                tls: unleash_edge_cli::TlsOptions {
                    tls_enable: false,
                    tls_server_key: None,
                    tls_server_cert: None,
                    tls_server_port: 3043,
                    redirect_http_to_https: false,
                },
                cors: unleash_edge_cli::CorsOptions {
                    cors_origin: None,
                    cors_allowed_headers: None,
                    cors_max_age: 172800,
                    cors_exposed_headers: None,
                    cors_methods: None,
                },
                allow_list: None,
                deny_list: None,
                workers: None,
            },
            mode: unleash_edge_cli::EdgeMode::default(),
            instance_id: "test-instance".to_string(),
            app_name: "unleash-edge-test".to_string(),
            markdown_help: false,
            trust_proxy: unleash_edge_cli::TrustProxy {
                trust_proxy: false,
                proxy_trusted_servers: vec![],
            },
            disable_all_endpoint: false,
            edge_request_timeout: 5,
            edge_keepalive_timeout: 5,
            log_format: unleash_edge_cli::LogFormat::Plain,
            auth_headers: unleash_edge_cli::AuthHeaders::default(),
            token_header: None,
            internal_backstage: unleash_edge_cli::InternalBackstageArgs {
                disable_metrics_batch_endpoint: false,
                disable_metrics_endpoint: false,
                disable_features_endpoint: false,
                disable_tokens_endpoint: false,
                disable_instance_data_endpoint: false,
            },
            sentry_config: unleash_edge_cli::SentryConfig {
                sentry_dsn: None,
                sentry_tracing_rate: 0.1,
                sentry_debug: false,
                sentry_enable_logs: false,
            },
            datadog_config: unleash_edge_cli::DatadogConfig { datadog_url: None },
            otel_config: unleash_edge_cli::OpenTelemetryConfig {
                otel_collector_url: None,
            },
            hosting_type: Some(Hosting::EnterpriseSelfHosted),
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

        let args = mock_cli_args();
        let edge_args = EdgeArgs {
            upstream_url: upstream.url(),
            tokens: vec!["*:development.hashyhashhash".to_string()],
            ..EdgeArgs::default()
        };

        let maybe_edge_state = build_edge_state(EdgeStateArgs {
            args,
            edge_args,
            client_meta_information,
            instances_observed_for_app_context,
            auth_headers: AuthHeaders::default(),
            http_client,
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

        let args = mock_cli_args();
        let edge_args = EdgeArgs {
            upstream_url: upstream.url(),
            tokens: vec!["*:development.hashyhashhash".to_string()],
            ..EdgeArgs::default()
        };

        let maybe_edge_state = build_edge_state(EdgeStateArgs {
            args,
            edge_args,
            client_meta_information,
            instances_observed_for_app_context,
            auth_headers: AuthHeaders::default(),
            http_client,
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
