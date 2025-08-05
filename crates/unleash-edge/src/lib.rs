use std::sync::Arc;
use axum::Router;
use chrono::Duration;
use dashmap::DashMap;
use ulid::Ulid;
use unleash_yggdrasil::EngineState;
use unleash_edge_appstate::AppState;
use unleash_edge_auth::token_validator::TokenValidator;
use unleash_edge_cli::{AuthHeaders, CliArgs, EdgeMode};
use unleash_edge_delta::cache_manager::DeltaCacheManager;
use unleash_edge_feature_cache::FeatureCache;
use unleash_edge_feature_refresh::FeatureRefresher;
use unleash_edge_http_client::{new_reqwest_client, ClientMetaInformation, HttpClientArgs};
use unleash_edge_http_client::instance_data::InstanceDataSending;
use unleash_edge_persistence::EdgePersistence;
use unleash_edge_types::EdgeResult;
use unleash_edge_types::metrics::instance_data::EdgeInstanceData;
use unleash_edge_types::tokens::EdgeToken;
use crate::edge_builder::build_edge;
use crate::offline_builder::build_offline;

pub mod edge_builder;
pub mod offline_builder;
pub mod health_checker;
pub mod ready_checker;
pub mod tls;

type CacheContainer = (
    Arc<DashMap<String, EdgeToken>>,
    Arc<FeatureCache>,
    Arc<DeltaCacheManager>,
    Arc<DashMap<String, EngineState>>,
);
pub type EdgeInfo = (
    CacheContainer,
    Option<Arc<TokenValidator>>,
    Option<Arc<FeatureRefresher>>,
    Option<Arc<dyn EdgePersistence>>,
);

pub async fn configure_server(args: CliArgs) -> EdgeResult<Router<AppState>> {
    let app_name = args.app_name.clone();
    let app_id: Ulid = Ulid::new();
    let edge_instance_data = Arc::new(EdgeInstanceData::new(&args.app_name, &app_id));
    let client_meta_information = ClientMetaInformation {
        app_name: args.app_name.clone(),
        instance_id: app_id.to_string(),
        connection_id: app_id.to_string(),
    };
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
                build_offline(offline_args.clone()).map(|cache| (cache, None, None, None))?;
            (caches, Arc::new(InstanceDataSending::SendNothing), None)
        }
        _ => unreachable!(),
    };
}