use crate::edge_builder::build_edge;
use crate::offline_builder::build_offline;
use axum::Router;
use axum::middleware::from_fn_with_state;
use axum::routing::get;
use chrono::Duration;
use std::env;
use std::pin::Pin;
use std::sync::{Arc, LazyLock};
use tokio::sync::RwLock;
use tower::ServiceBuilder;
use ulid::Ulid;
use unleash_edge_appstate::AppState;
use unleash_edge_auth::token_validator::TokenValidator;
use unleash_edge_cli::{AuthHeaders, CliArgs, EdgeArgs, EdgeMode};
use unleash_edge_delta::cache_manager::DeltaCacheManager;
use unleash_edge_feature_cache::FeatureCache;
use unleash_edge_feature_refresh::FeatureRefresher;
use unleash_edge_http_client::instance_data::{InstanceDataSending, loop_send_instance_data};
use unleash_edge_http_client::{new_reqwest_client, ClientMetaInformation, HttpClientArgs, UnleashClient};
use unleash_edge_metrics::axum_prometheus_metrics::{
    PrometheusAxumLayer, render_prometheus_metrics,
};
use unleash_edge_metrics::{metrics_pusher, send_unleash_metrics};
use unleash_edge_persistence::{EdgePersistence, persist_data};
use unleash_edge_types::metrics::MetricsCache;
use unleash_edge_types::metrics::instance_data::EdgeInstanceData;
use unleash_edge_types::{EdgeResult, EngineCache, TokenCache};

pub mod edge_builder;
pub mod health_checker;
mod middleware;
pub mod offline_builder;
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
            let caches = build_offline(offline_args.clone())
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
        .merge(unleash_edge_frontend_api::router(args.disable_all_endpoint))
        .layer(
            ServiceBuilder::new()
                .layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::validate_token::validate_token,
                ))
                .layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::consumption::connection_consumption,
                )),
        );

    let top_router: Router = Router::new()
        .nest("/api", api_router)
        .nest("/edge", unleash_edge_edge_api::router())
        .nest(
            "/internal-backstage",
            Router::new()
                .route("/metrics", get(render_prometheus_metrics))
                .merge(unleash_edge_backstage::router(args.internal_backstage)),
        )
        .layer(
            ServiceBuilder::new()
                .layer(metrics_middleware)
                .layer(args.http.cors.middleware())
                .layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::deny_list::deny_middleware,
                ))
                .layer(from_fn_with_state(
                    app_state.clone(),
                    middleware::allow_list::allow_middleware,
                )),
        )
        .with_state(app_state);
    Ok(top_router)
}

fn spawn_background_tasks(
    edge: EdgeArgs,
    client_meta_information: ClientMetaInformation,
    feature_refresher: Arc<FeatureRefresher>,
    persistence: Option<Arc<dyn EdgePersistence>>,
    lazy_token_cache: Arc<TokenCache>,
    lazy_feature_cache: Arc<FeatureCache>,
    validator: Arc<TokenValidator>,
    lazy_feature_refresher: Option<Arc<FeatureRefresher>>,
    http_client: reqwest::Client,
    registry: prometheus::Registry,
    app_name: String,
    instance_data_sender: Arc<InstanceDataSending>,
    edge_instance_data: Arc<EdgeInstanceData>,
    instances_observed_for_app_context: Arc<RwLock<Vec<EdgeInstanceData>>>,
    metrics_cache_clone: Arc<MetricsCache>,
    token_cache: Arc<TokenCache>,
    unleash_client: Arc<UnleashClient>,
) {
    tokio::spawn(spawn_fetch_task(
        &edge,
        client_meta_information,
        feature_refresher,
    ));

    tokio::spawn(async move {
        send_unleash_metrics::send_metrics_task(
            metrics_cache_clone.clone(),
            unleash_client,
            token_cache.clone(),
            edge.metrics_interval_seconds.try_into().unwrap(),
        )
        .await;
    });
    tokio::spawn(async move {
        persist_data(
            persistence.clone(),
            lazy_token_cache.clone(),
            lazy_feature_cache.clone(),
        )
        .await;
    });

    let validator_clone = validator.clone();
    tokio::spawn(async move {
        validator_clone
            .schedule_validation_of_known_tokens(edge.token_revalidation_interval_seconds)
            .await
    });

    let validator = validator.clone();
    tokio::spawn(async move {
        validator
            .schedule_revalidation_of_startup_tokens(
                edge.tokens.clone(),
                lazy_feature_refresher.clone(),
            )
            .await
    });

    tokio::spawn(async move {
        metrics_pusher::prometheus_remote_write(
            http_client,
            registry,
            edge.prometheus_remote_write_url.clone(),
            edge.prometheus_push_interval,
            app_name,
        )
        .await
    });

    tokio::spawn(async move {
        loop_send_instance_data(
            instance_data_sender.clone(),
            edge_instance_data.clone(),
            instances_observed_for_app_context.clone(),
        )
    });
}

fn spawn_fetch_task(
    edge: &EdgeArgs,
    client_meta_information: ClientMetaInformation,
    feature_refresher: Arc<FeatureRefresher>,
) -> Pin<Box<dyn Future<Output = ()> + Send>> {
    let refresher_for_background = feature_refresher.clone();

    if edge.streaming {
        let custom_headers = edge.custom_client_headers.clone();
        if edge.delta {
            Box::pin(async move {
                let _ = refresher_for_background
                    .start_streaming_delta_background_task(client_meta_information, custom_headers)
                    .await;
            })
        } else {
            Box::pin(async move {
                let _ = refresher_for_background
                    .start_streaming_features_background_task(
                        client_meta_information,
                        custom_headers,
                    )
                    .await;
            })
        }
    } else {
        Box::pin(async move {
            feature_refresher
                .start_refresh_features_background_task()
                .await;
        })
    }
}
