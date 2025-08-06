use crate::edge_builder::build_edge;
use crate::offline_builder::build_offline;
use axum::middleware::from_fn_with_state;
use axum::Router;
use chrono::Duration;
use std::env;
use std::sync::{Arc, LazyLock};
use axum::routing::get;
use tower::ServiceBuilder;
use ulid::Ulid;
use unleash_edge_appstate::AppState;
use unleash_edge_auth::token_validator::TokenValidator;
use unleash_edge_cli::{AuthHeaders, CliArgs, EdgeMode};
use unleash_edge_delta::cache_manager::DeltaCacheManager;
use unleash_edge_feature_cache::FeatureCache;
use unleash_edge_feature_refresh::FeatureRefresher;
use unleash_edge_http_client::instance_data::InstanceDataSending;
use unleash_edge_http_client::{new_reqwest_client, ClientMetaInformation, HttpClientArgs};
use unleash_edge_metrics::axum_prometheus_metrics::{render_prometheus_metrics, PrometheusAxumLayer};
use unleash_edge_persistence::EdgePersistence;
use unleash_edge_types::metrics::instance_data::EdgeInstanceData;
use unleash_edge_types::{EdgeResult, EngineCache, TokenCache};

mod middleware;
pub mod edge_builder;
pub mod offline_builder;
pub mod health_checker;
pub mod ready_checker;
pub mod tls;
pub mod tracing;

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
    Arc<Option<TokenValidator>>,
    Arc<Option<FeatureRefresher>>,
    Option<Arc<dyn EdgePersistence>>,
);

pub async fn configure_server(args: CliArgs) -> EdgeResult<Router> {
    let app_id: Ulid = Ulid::new();
    let edge_instance_data = Arc::new(EdgeInstanceData::new(&args.app_name, &app_id));
    let client_meta_information = ClientMetaInformation {
        app_name: args.app_name.clone(),
        instance_id: app_id.to_string(),
        connection_id: app_id.to_string(),
    };
    let metrics_middleware = PrometheusAxumLayer::new();
    let (edge_info, instance_data_sender, token_validation_queue) = match &args.mode {
        EdgeMode::Edge(edge_args) => {
            let client = new_reqwest_client(HttpClientArgs {
                skip_ssl_verification: edge_args.skip_ssl_verification,
                client_identity: edge_args.client_identity.clone(),
                upstream_certificate_file: edge_args.upstream_certificate_file.clone(),
                connect_timeout: Duration::seconds(edge_args.upstream_request_timeout),
                socket_timeout: Duration::seconds(edge_args.upstream_socket_timeout),
                keep_alive_timeout: Duration::seconds(edge_args.client_keepalive_timeout),
                client_meta_information: client_meta_information.clone(),
            })?;

            let (deferred_validation_tx, deferred_validation_rx) = if *SHOULD_DEFER_VALIDATION {
                let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
                (Some(tx), Some(rx))
            } else {
                (None, None)
            };

            let auth_headers = AuthHeaders::from(&args);
            let caches = build_edge(
                edge_args,
                client_meta_information.clone(),
                auth_headers,
                client.clone(),
                deferred_validation_tx,
            )
                .await?;

            let instance_data_sender: Arc<InstanceDataSending> =
                Arc::new(InstanceDataSending::from_args(
                    args.clone(),
                    &client_meta_information,
                    client,
                    metrics_middleware.registry.clone(),
                )?);

            (caches, instance_data_sender, deferred_validation_rx)
        }
        EdgeMode::Offline(offline_args) => {
            let caches =
                build_offline(offline_args.clone())
                    .map(|cache| (cache, Arc::new(None), Arc::new(None), None))?;
            (caches, Arc::new(InstanceDataSending::SendNothing), None)
        }
        _ => unreachable!(),
    };
    let (
        (token_cache, features_cache, _, engine_cache),
        token_validator,
        feature_refresher,
        persistence,
    ) = edge_info.clone();
    let app_state = AppState::builder()
        .with_token_cache(token_cache.clone())
        .with_features_cache(features_cache.clone())
        .with_engine_cache(engine_cache.clone())
        .with_token_validator(token_validator.clone())
        .with_feature_refresher(feature_refresher.clone())
        .with_persistence(persistence)
        .with_deny_list(args.http.deny_list.unwrap_or_default())
        .with_allow_list(args.http.allow_list.unwrap_or_default())
        .with_instance_sending(instance_data_sender)
        .with_edge_instance_data(edge_instance_data)
        .build();
    let api_router = Router::new()
            .nest("/client", unleash_edge_client_api::router())
            .nest("/frontend", unleash_edge_frontend_api::router(args.disable_all_endpoint))
            .layer(ServiceBuilder::new()
                       .layer(from_fn_with_state(app_state.clone(), middleware::validate_token::validate_token))
                       .layer(from_fn_with_state(app_state.clone(), middleware::consumption::connection_consumption))
            );

    let top_router: Router = Router::new()
        .nest("/api", api_router)
        .nest("/edge", unleash_edge_edge_api::router())
        .nest("/internal-backstage", Router::new()
                        .route("/metrics", get(render_prometheus_metrics))
                        .merge(unleash_edge_backstage::router(args.internal_backstage))
        )
        .layer(ServiceBuilder::new()
            .layer(metrics_middleware)
            .layer(args.http.cors.middleware())
            .layer(from_fn_with_state(app_state.clone(), middleware::deny_list::deny_middleware))
            .layer(from_fn_with_state(app_state.clone(), middleware::allow_list::allow_middleware))
        )
        .with_state(app_state);
    Ok(top_router)
}