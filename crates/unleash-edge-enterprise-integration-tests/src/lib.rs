#[cfg(test)]
mod tests {

    use axum::Router;
    use axum::routing::post;
    use axum_test::TestServer;
    use chrono::Duration;
    use reqwest::Client;
    use std::sync::Arc;
    use tokio::sync::RwLock;
    use ulid::Ulid;
    use unleash_edge::edge_builder::build_edge_state;
    use unleash_edge_cli::{AuthHeaders, CliArgs, EdgeArgs, HttpServerArgs};
    use unleash_edge_types::errors::EdgeError;
    use unleash_edge_types::metrics::instance_data::EdgeInstanceData;

    use unleash_edge_http_client::{ClientMetaInformation, HttpClientArgs, new_reqwest_client};

    use unleash_edge_types::EdgeResult;

    pub async fn heartbeat() -> EdgeResult<()> {
        Err(EdgeError::Forbidden("This Edge instance is not licensed! The police will arrive at your doorstep imminently".into()))
    }

    fn build_license_heartbeat_router() -> Router {
        Router::new().route("/edge-licensing/heartbeat", post(heartbeat))
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
        }
    }

    fn build_edge_state_data() -> (
        Client,
        Arc<EdgeInstanceData>,
        ClientMetaInformation,
        Arc<RwLock<Vec<EdgeInstanceData>>>,
    ) {
        let client_meta_information = ClientMetaInformation {
            app_name: "unleash-edge-test".to_string(),
            connection_id: "test-connection-id".to_string(),
            instance_id: "test-instance-id".to_string(),
        };

        let edge_instance_data = Arc::new(EdgeInstanceData::new(
            "cheese-shop".into(),
            &Ulid::new(),
            None,
        ));

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
            edge_instance_data,
            client_meta_information,
            instances_observed_for_app_context,
        )
    }

    async fn test_upstream_server() -> TestServer {
        let router = Router::new().nest("/api/client", build_license_heartbeat_router());
        TestServer::builder()
            .http_transport()
            .build(router)
            .expect("Failed to build client api test server")
    }

    #[tokio::test]
    async fn enterprise_edge_state_errors_with_forbidden_when_license_is_not_retrievable() {
        let server = test_upstream_server().await;
        let (
            http_client,
            edge_instance_data,
            client_meta_information,
            instances_observed_for_app_context,
        ) = build_edge_state_data();

        let cli_args = mock_cli_args();
        let edge_args = EdgeArgs {
            upstream_url: server.server_url("/").unwrap().to_string(),
            tokens: vec!["*:development.hashyhashhash".to_string()],
            ..EdgeArgs::default()
        };

        let maybe_edge_state = build_edge_state(
            cli_args,
            &edge_args,
            client_meta_information,
            edge_instance_data,
            instances_observed_for_app_context,
            AuthHeaders::default(),
            http_client,
        )
        .await;

        if let Err(maybe_err) = &maybe_edge_state {
            println!("Edge state build failed as expected: {:#?}", maybe_err);
        }

        assert!(matches!(maybe_edge_state, Err(EdgeError::Forbidden(_))));
    }

    #[tokio::test]
    async fn enterprise_edge_state_errors_with_heartbeat_error_when_license_cannot_be_verified() {
        let server = test_upstream_server().await;
        let (
            http_client,
            edge_instance_data,
            client_meta_information,
            instances_observed_for_app_context,
        ) = build_edge_state_data();

        let cli_args = mock_cli_args();
        let edge_args = EdgeArgs {
            upstream_url: server
                .server_url("/bad-url-that-should-make-endpoints-404")
                .unwrap()
                .to_string(),
            tokens: vec!["*:development.hashyhashhash".to_string()],
            ..EdgeArgs::default()
        };

        let maybe_edge_state = build_edge_state(
            cli_args,
            &edge_args,
            client_meta_information,
            edge_instance_data,
            instances_observed_for_app_context,
            AuthHeaders::default(),
            http_client,
        )
        .await;

        if let Err(maybe_err) = &maybe_edge_state {
            println!("Edge state build failed as expected: {:#?}", maybe_err);
        }

        assert!(matches!(
            maybe_edge_state,
            Err(EdgeError::HeartbeatError(_, _))
        ));
    }
}
