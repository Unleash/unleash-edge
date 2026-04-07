use crate::edge_builder::{EdgeStateArgs, PersistenceArgs, build_edge_state};
use crate::offline_builder::build_offline_app_state;
use ::tracing::info;
use axum::Router;
use axum::middleware::{from_fn, from_fn_with_state};
use axum::routing::get;
use chrono::Duration;
use unleash_edge_appstate::AppState;
#[cfg(feature = "enterprise")]
use unleash_edge_enterprise_api::heartbeat;

use reqwest::Client;
use std::env;
use std::sync::{Arc, LazyLock, OnceLock};
use tokio::sync::RwLock;

use tower::ServiceBuilder;
use tower_http::compression::CompressionLayer;
use tracing::warn;
use ulid::Ulid;
use unleash_edge_auth::token_validator::TokenValidator;
use unleash_edge_cli::{AuthHeaders, CliArgs, EdgeMode, HmacConfig};
use unleash_edge_delta::cache_manager::DeltaCacheManager;
use unleash_edge_feature_cache::FeatureCache;
use unleash_edge_feature_refresh::HydratorType;
use unleash_edge_http_client::{ClientMetaInformation, HttpClientArgs, new_reqwest_client};
use unleash_edge_metrics::axum_prometheus_metrics::{
    PrometheusAxumLayer, render_prometheus_metrics,
};
use unleash_edge_persistence::EdgePersistence;
use unleash_edge_request_logger::log_request_middleware;
use unleash_edge_tracing::OtelHolder;
use unleash_edge_types::errors::EdgeError;
use unleash_edge_types::metrics::instance_data::{EdgeInstanceData, Hosting};
use unleash_edge_types::tokens::EdgeToken;
use unleash_edge_types::urls::UnleashUrls;
use unleash_edge_types::{BackgroundTask, EdgeResult, EngineCache, TokenCache};
use url::Url;

pub mod edge_builder;
pub mod health_checker;
pub mod middleware;
pub mod offline_builder;
pub mod ready_checker;
pub mod tls;

static SHOULD_DEFER_VALIDATION: LazyLock<bool> = LazyLock::new(|| {
    env::var("EDGE_DEFER_TOKEN_VALIDATION")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false)
});

type CacheContainer = (
    Arc<TokenCache>,
    Arc<FeatureCache>,
    Arc<DeltaCacheManager>,
    Arc<EngineCache>,
);
pub type EdgeInfo = (
    CacheContainer,
    Arc<TokenValidator>,
    HydratorType,
    Option<Arc<dyn EdgePersistence>>,
);

static OTEL_INIT: OnceLock<Arc<Option<OtelHolder>>> = OnceLock::new();

#[cfg(feature = "enterprise")]
const DEFAULT_HOSTING: Hosting = Hosting::EnterpriseSelfHosted;

#[cfg(not(feature = "enterprise"))]
const DEFAULT_HOSTING: Hosting = Hosting::SelfHosted;

pub async fn build_tokens(
    http_client: Client,
    upstream_url: Url,
    tokens: Vec<EdgeToken>,
    hmac_config: HmacConfig,
) -> EdgeResult<Vec<EdgeToken>> {
    if let Some(token_request) = hmac_config.possible_token_request(
        http_client,
        UnleashUrls::from_base_url(upstream_url).token_request_url,
    ) {
        let unleash_granted_tokens =
            unleash_edge_http_client::token_request::request_tokens(token_request).await;
        if !tokens.is_empty() {
            warn!(
                "Both tokens and hmac_config were configured. Overriding startup tokens with tokens obtained via hmac_config."
            );
        }
        unleash_granted_tokens
    } else if !tokens.is_empty() {
        Ok(tokens)
    } else {
        Ok(vec![])
    }
}

pub async fn configure_server(args: CliArgs) -> EdgeResult<(Router, Vec<BackgroundTask>)> {
    let app_id: Ulid = Ulid::new();
    let client_meta_information = ClientMetaInformation {
        app_name: args.app_name.clone(),
        instance_id: app_id,
        connection_id: app_id,
    };
    let client_id = args.client_id.clone().unwrap_or("self-hosted".into());

    let instances_observed_for_app_context: Arc<RwLock<Vec<EdgeInstanceData>>> =
        Arc::new(RwLock::new(Vec::new()));
    let metrics_middleware =
        PrometheusAxumLayer::new(&args.app_name.clone(), &app_id.clone().to_string());

    let (app_state, background_tasks, shutdown_tasks) = match &args.mode {
        EdgeMode::Edge(edge_args) => {
            let upstream_url = Url::parse(&edge_args.upstream_url)
                .map_err(|_e| EdgeError::InvalidServerUrl(edge_args.upstream_url.clone()))?;
            let http_client = new_reqwest_client(HttpClientArgs {
                skip_ssl_verification: edge_args.skip_ssl_verification,
                client_identity: edge_args.client_identity.clone(),
                upstream_certificate_file: edge_args.upstream_certificate_file.clone(),
                connect_timeout: Duration::seconds(edge_args.upstream_request_timeout),
                socket_timeout: Duration::seconds(edge_args.upstream_socket_timeout),
                keep_alive_timeout: Duration::seconds(edge_args.client_keepalive_timeout),
                client_meta_information: client_meta_information.clone(),
            })?;

            let tokens = build_tokens(
                http_client.clone(),
                upstream_url.clone(),
                edge_args.tokens.clone(),
                edge_args.hmac_config.clone(),
            )
            .await?;

            let auth_headers = AuthHeaders::from(&args);

            build_edge_state(EdgeStateArgs {
                client_meta_information,
                instances_observed_for_app_context: instances_observed_for_app_context.clone(),
                auth_headers,
                http_client,
                hosting_type: args.hosting_type.unwrap_or(DEFAULT_HOSTING),
                client_id,
                app_id,
                otel_endpoint_url: args.otel_config.otel_exporter_otlp_endpoint,
                otel_protocol: args.otel_config.otel_exporter_otlp_protocol,
                tokens,
                base_path: args.http.base_path.clone(),
                http_deny_list: args.http.deny_list,
                log_format: args.log_format,
                upstream_url,
                custom_client_headers: edge_args.custom_client_headers.clone(),
                http_allow_list: args.http.allow_list,
                #[cfg(feature = "enterprise")]
                streaming: edge_args.streaming,
                #[cfg(not(feature = "enterprise"))]
                streaming: false,
                #[cfg(feature = "enterprise")]
                delta: edge_args.delta,
                #[cfg(not(feature = "enterprise"))]
                delta: false,
                persistence_args: PersistenceArgs {
                    s3: edge_args.s3.clone(),
                    redis: edge_args.redis.clone(),
                    backup_folder: edge_args.backup_folder.clone(),
                },
                pretrusted_tokens: edge_args.pretrusted_tokens.clone(),
                features_refresh_interval: Duration::seconds(
                    edge_args.features_refresh_interval_seconds as i64,
                ),

                metrics_interval_seconds: edge_args.metrics_interval_seconds as i64,
                token_revalidation_interval_seconds: edge_args.token_revalidation_interval_seconds,
                prometheus_remote_write_url: edge_args.prometheus_remote_write_url.clone(),
                prometheus_push_interval: edge_args.prometheus_push_interval,
                prometheus_username: edge_args.prometheus_username.clone(),
                prometheus_password: edge_args.prometheus_password.clone(),
                hostname: edge_args.hostname.clone(),
                ec2_instance_id: edge_args.ec2_instance_id.clone(),
            })
            .await?
        }
        EdgeMode::Offline(offline_args) => {
            build_offline_app_state(args.clone(), offline_args.clone()).await?
        }
        _ => unreachable!(),
    };

    for task in background_tasks {
        tokio::spawn(task);
    }
    let api_router = Router::new()
        .nest("/client", build_edge_router())
        .merge(unleash_edge_frontend_api::router(args.disable_all_endpoint))
        .layer(
            ServiceBuilder::new()
                .layer(from_fn(log_request_middleware))
                .layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::validate_token::validate_token,
                ))
                .layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::consumption::connection_consumption,
                ))
                .layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::consumption::request_consumption,
                ))
                .layer(from_fn(middleware::etag::etag_middleware)),
        );

    let backstage_router = if !args.internal_backstage.disable_metrics_endpoint {
        Router::new()
            .route("/metrics", get(render_prometheus_metrics))
            .merge(unleash_edge_backstage::router(
                args.internal_backstage.clone(),
            ))
    } else {
        unleash_edge_backstage::router(args.internal_backstage.clone())
    };

    let top_router: Router = Router::new()
        .nest("/api", api_router)
        .nest("/edge", unleash_edge_edge_api::router())
        .nest("/internal-backstage", backstage_router)
        .layer(
            ServiceBuilder::new()
                .layer(CompressionLayer::new())
                .layer(metrics_middleware)
                .layer(args.http.cors.middleware())
                .layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::deny_list::deny_middleware,
                ))
                .layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::allow_list::allow_middleware,
                ))
                .layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::client_metrics::extract_request_metrics,
                ))
                .layer(tower_http::trace::TraceLayer::new_for_http()),
        )
        .with_state(app_state);

    let router_to_host = if args.http.base_path.len() > 1 {
        info!("Had a path different from root. Setting up a nested router");
        let path = if !args.http.base_path.starts_with("/") {
            format!("/{}", args.http.base_path)
        } else {
            args.http.base_path.clone()
        };
        Router::new().nest(&path, top_router)
    } else {
        top_router
    };

    Ok((router_to_host, shutdown_tasks))
}

#[cfg(feature = "enterprise")]
pub fn build_edge_router() -> Router<AppState> {
    unleash_edge_client_api::router().merge(heartbeat::router())
}

#[cfg(not(feature = "enterprise"))]
pub fn build_edge_router() -> Router<AppState> {
    unleash_edge_client_api::router()
}
