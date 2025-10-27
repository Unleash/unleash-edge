#[cfg(test)]
mod tests {
    use axum::Json;
    use axum::extract::State;
    use axum::routing::post;
    use axum::{Router, response::IntoResponse};
    use axum_test::TestServer;
    use chrono::Duration;
    use reqwest::Client;
    use serde_json::json;
    use std::sync::Arc;
    use tokio::sync::RwLock;
    use ulid::Ulid;
    use unleash_edge::edge_builder::{EdgeStateArgs, build_edge_state};
    use unleash_edge_cli::{AuthHeaders, CliArgs, EdgeArgs, HttpServerArgs};
    use unleash_edge_types::errors::EdgeError;
    use unleash_edge_types::metrics::instance_data::{EdgeInstanceData, Hosting};

    use unleash_edge_http_client::{ClientMetaInformation, HttpClientArgs, new_reqwest_client};

    use unleash_edge_types::EdgeResult;

    #[derive(Clone)]
    struct MockState {
        license_result: Arc<RwLock<EdgeResult<()>>>,
    }

    pub struct UpstreamMock {
        server: TestServer,
    }

    impl UpstreamMock {
        pub async fn new(initial: EdgeResult<()>) -> Self {
            let state = MockState {
                license_result: Arc::new(RwLock::new(initial)),
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

        async fn heartbeat(State(s): State<MockState>) -> EdgeResult<()> {
            s.license_result.read().await.clone()
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

        let http_client = new_reqwest_client(HttpClientArgs {
            skip_ssl_verification: false,
            client_identity: None,
            upstream_certificate_file: None,
            connect_timeout: Duration::seconds(10),
            socket_timeout: Duration::seconds(10),
            keep_alive_timeout: Duration::seconds(10),
            client_meta_information: client_meta_information.clone(),
        })
        .unwrap();

        let instances_observed_for_app_context = Arc::new(RwLock::new(vec![]));

        (
            http_client,
            client_meta_information,
            instances_observed_for_app_context,
        )
    }

    #[tokio::test]
    async fn enterprise_edge_state_errors_with_invalid_license_when_license_is_not_retrievable() {
        let upstream = UpstreamMock::new(Err(EdgeError::InvalidLicense("This Edge instance is not licensed! The police will arrive at your doorstep imminently".into()))).await;
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
            Err(EdgeError::InvalidLicense(_))
        ));
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
        let upstream = UpstreamMock::new(Ok(())).await;
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
}
