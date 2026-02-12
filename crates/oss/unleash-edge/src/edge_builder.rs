use crate::{CacheContainer, EdgeInfo, OTEL_INIT, SHOULD_DEFER_VALIDATION};
use chrono::{Duration, Utc};
use dashmap::DashMap;
use http::StatusCode;
use std::pin::Pin;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::watch::{Receiver, channel};
use tracing::{debug, error, info, warn};
use unleash_edge_appstate::AppState;
use unleash_edge_appstate::token_cache_observer::observe_tokens_in_background;
use unleash_edge_auth::token_validator::{
    TokenValidator, create_deferred_validation_task, create_revalidation_of_startup_tokens_task,
    create_revalidation_task,
};
use unleash_edge_cli::{AuthHeaders, CliArgs, EdgeArgs, RedisMode};
use unleash_edge_delta::cache_manager::{DeltaCacheManager, create_terminate_sse_connections_task};
use unleash_edge_feature_cache::FeatureCache;
use unleash_edge_feature_refresh::delta_refresh::{
    DeltaRefresher, start_streaming_delta_background_task,
};
use unleash_edge_feature_refresh::{
    FeatureRefreshConfig, FeatureRefresher, HydratorType, start_refresh_features_background_task,
};
use unleash_edge_http_client::instance_data::{
    InstanceDataSending, create_once_off_send_instance_data, create_send_instance_data_task,
};
use unleash_edge_http_client::{ClientMetaInformation, UnleashClient};
use unleash_edge_metrics::metrics_pusher::{PrometheusWriteTaskArgs, create_prometheus_write_task};
use unleash_edge_metrics::send_unleash_metrics::{
    create_once_off_send_metrics, create_send_metrics_task,
};
use unleash_edge_persistence::file::FilePersister;
use unleash_edge_persistence::redis::RedisPersister;
#[cfg(feature = "s3-persistence")]
use unleash_edge_persistence::s3::s3_persister::S3Persister;
use unleash_edge_persistence::{
    EdgePersistence, create_once_off_persist, create_persist_data_task,
};
use unleash_edge_tracing::{init_tracing_and_logging, shutdown_logging};
use unleash_edge_types::enterprise::{ApplicationLicenseState, LicenseState};
use unleash_edge_types::errors::EdgeError;
use unleash_edge_types::metrics::MetricsCache;
use unleash_edge_types::metrics::instance_data::{EdgeInstanceData, Hosting};
use unleash_edge_types::tokens::{EdgeToken, cache_key};
use unleash_edge_types::urls::UnleashUrls;
use unleash_edge_types::{
    BackgroundTask, EdgeResult, EngineCache, RefreshState, TokenCache, TokenType,
    TokenValidationStatus,
};
use unleash_types::client_metrics::ConnectVia;
use unleash_yggdrasil::{EngineState, UpdateMessage};
use url::Url;

#[cfg(feature = "enterprise")]
const DEFAULT_HOSTING: Hosting = Hosting::EnterpriseSelfHosted;

#[cfg(not(feature = "enterprise"))]
const DEFAULT_HOSTING: Hosting = Hosting::SelfHosted;

pub struct EdgeStateArgs {
    pub args: CliArgs,
    pub edge_args: EdgeArgs,
    pub client_meta_information: ClientMetaInformation,
    pub instances_observed_for_app_context: Arc<RwLock<Vec<EdgeInstanceData>>>,
    pub auth_headers: AuthHeaders,
    pub http_client: reqwest::Client,
}

pub fn build_caches() -> CacheContainer {
    let token_cache: TokenCache = DashMap::default();
    let features_cache: FeatureCache = FeatureCache::new(DashMap::default());
    let delta_cache_manager = DeltaCacheManager::new();
    let engine_cache: EngineCache = DashMap::default();
    (
        Arc::new(token_cache),
        Arc::new(features_cache),
        Arc::new(delta_cache_manager),
        Arc::new(engine_cache),
    )
}

async fn get_data_source(args: &EdgeArgs) -> Option<Arc<dyn EdgePersistence>> {
    if let Some(redis_args) = args.redis.clone() {
        let mut filtered_redis_args = redis_args.clone();
        if filtered_redis_args.redis_password.is_some() {
            filtered_redis_args.redis_password = Some("[redacted]".to_string());
        }
        debug!("Configuring Redis persistence {filtered_redis_args:?}");
        let redis_persister = match redis_args.redis_mode {
            RedisMode::Single => redis_args.to_url().map(|url| {
                RedisPersister::new(&url, redis_args.read_timeout(), redis_args.write_timeout())
                    .expect("Failed to connect to redis")
            }),
            RedisMode::Cluster => redis_args.redis_url.clone().map(|urls| {
                RedisPersister::new_with_cluster(
                    urls,
                    redis_args.read_timeout(),
                    redis_args.write_timeout(),
                )
                .expect("Failed to connect to redis cluster")
            }),
        }
        .unwrap_or_else(|| {
            panic!(
                "Could not build a redis persister from redis_args {:?}",
                args.redis
            )
        });
        return Some(Arc::new(redis_persister));
    }
    #[cfg(feature = "s3-persistence")]
    if let Some(s3_args) = args.s3.clone() {
        let s3_persister = S3Persister::new_from_env(
            &s3_args
                .s3_bucket_name
                .clone()
                .expect("Clap is confused, there's no bucket name"),
        )
        .await;
        return Some(Arc::new(s3_persister));
    }

    if let Some(backup_folder) = args.backup_folder.clone() {
        debug!("Configuring file persistence {backup_folder:?}");
        let backup_client = FilePersister::new(&backup_folder);
        return Some(Arc::new(backup_client));
    }

    None
}

async fn hydrate_from_persistent_storage(cache: CacheContainer, storage: Arc<dyn EdgePersistence>) {
    let (token_cache, features_cache, _delta_cache, engine_cache) = cache;
    // TODO: do we need to hydrate from persistent storage for delta?
    let tokens = storage.load_tokens().await.unwrap_or_else(|error| {
        warn!("Failed to load tokens from cache {error:?}");
        vec![]
    });
    let features = storage.load_features().await.unwrap_or_else(|error| {
        warn!("Failed to load features from cache {error:?}");
        Default::default()
    });
    for token in tokens {
        debug!("Hydrating tokens {token:?}");
        token_cache.insert(token.token.clone(), token);
    }

    for (key, features) in features {
        debug!("Hydrating features for {key:?}");
        features_cache.insert(key.clone(), features.clone());
        let mut engine_state = EngineState::default();

        let warnings = engine_state.take_state(UpdateMessage::FullResponse(features));
        if let Some(warnings) = warnings {
            warn!("Failed to hydrate features for {key:?}: {warnings:?}");
        }
        engine_cache.insert(key.clone(), engine_state);
    }
}

pub async fn build_edge(
    args: &EdgeArgs,
    client_meta_information: ClientMetaInformation,
    edge_instance_data: Arc<EdgeInstanceData>,
    auth_headers: AuthHeaders,
    http_client: reqwest::Client,
    tx: Option<UnboundedSender<String>>,
) -> EdgeResult<EdgeInfo> {
    if args.tokens.is_empty() {
        return Err(EdgeError::NoTokens(
            "No tokens provided. Tokens must be specified".into(),
        ));
    }
    let (token_cache, feature_cache, delta_cache, engine_cache) = build_caches();
    let persistence = get_data_source(args).await;
    args.tokens.iter().for_each(|token| {
        if token.status == TokenValidationStatus::Validated {
            token_cache.insert(token.token.clone(), token.clone());
        }
    });
    let unleash_client = Url::parse(&args.upstream_url.clone())
        .map(|url| {
            UnleashClient::from_url_with_backing_client(
                url,
                auth_headers
                    .upstream_auth_header
                    .clone()
                    .unwrap_or("Authorization".to_string()),
                http_client,
                client_meta_information.clone(),
            )
        })
        .map(|c| c.with_custom_client_headers(args.custom_client_headers.clone()))
        .map(Arc::new)
        .map_err(|_| EdgeError::InvalidServerUrl(args.upstream_url.clone()))?;

    if let Some(token_pairs) = &args.pretrusted_tokens {
        for (token_string, trusted_token) in token_pairs {
            token_cache.insert(token_string.clone(), trusted_token.clone());
        }
    }

    let token_validator = TokenValidator::new_lazy(
        unleash_client.clone(),
        token_cache.clone(),
        persistence.clone(),
        tx,
    );

    let delta_cache_manager = Arc::new(DeltaCacheManager::new());
    let feature_config = FeatureRefreshConfig::new(
        Duration::seconds(args.features_refresh_interval_seconds as i64),
        client_meta_information.clone(),
    );

    let hydrator_type = load_hydrator(LoadHydratorArgs {
        args,
        unleash_client: unleash_client.clone(),
        feature_cache: feature_cache.clone(),
        delta_cache_manager: delta_cache_manager.clone(),
        engine_cache: engine_cache.clone(),
        persistence: persistence.clone(),
        feature_config: &feature_config,
        client_meta_information: &client_meta_information,
        edge_instance_data: edge_instance_data.clone(),
    });

    let _ = token_validator
        .register_tokens(args.tokens.clone().into_iter().map(|t| t.token).collect())
        .await;
    if let Some(persistence) = persistence.clone() {
        hydrate_from_persistent_storage(
            (
                token_cache.clone(),
                feature_cache.clone(),
                delta_cache.clone(),
                engine_cache.clone(),
            ),
            persistence,
        )
        .await;
    }
    if token_cache.is_empty() {
        error!("Edge was not able to validate any of the tokens configured at startup");
        return Err(EdgeError::NoTokens("No valid tokens provided on startup. At least one valid token must be specified at startup".into()));
    }
    for validated_token in token_cache
        .iter()
        .filter(|candidate| candidate.value().token_type == Some(TokenType::Backend))
    {
        hydrator_type
            .register_token_for_refresh(validated_token.clone(), None)
            .await;
        if let Some(env) = validated_token.environment.as_ref() {
            edge_instance_data.observe_api_key_refresh(
                env.clone(),
                validated_token.projects.clone(),
                feature_cache
                    .get(&cache_key(&validated_token))
                    .map(|f| f.value().clone())
                    .and_then(|features| features.meta.and_then(|meta| meta.revision_id))
                    .unwrap_or(0),
                Utc::now(),
            );
        }
    }
    hydrator_type.hydrate_new_tokens().await;
    Ok((
        (
            token_cache,
            feature_cache,
            delta_cache_manager,
            engine_cache,
        ),
        Arc::new(token_validator),
        hydrator_type,
        persistence,
    ))
}

pub async fn build_edge_state(
    EdgeStateArgs {
        args,
        edge_args,
        client_meta_information,
        instances_observed_for_app_context,
        auth_headers,
        http_client,
    }: EdgeStateArgs,
) -> EdgeResult<(AppState, Vec<BackgroundTask>, Vec<BackgroundTask>)> {
    let edge_instance_data = Arc::new(EdgeInstanceData::new(
        &client_meta_information.app_name,
        &client_meta_information.instance_id,
        args.hosting_type.or(Some(DEFAULT_HOSTING)),
    ));

    OTEL_INIT.get_or_init(|| {
        Arc::new(
            init_tracing_and_logging(
                &args,
                client_meta_information.instance_id.clone().to_string(),
            )
            .unwrap_or_default(),
        )
    });
    let mut edge_args = edge_args.clone();

    let unleash_client = Url::parse(&edge_args.upstream_url.clone())
        .map(|url| {
            UnleashClient::from_url_with_backing_client(
                url,
                auth_headers
                    .upstream_auth_header
                    .clone()
                    .unwrap_or("Authorization".to_string()),
                http_client.clone(),
                client_meta_information.clone(),
            )
        })
        .map(|c| c.with_custom_client_headers(edge_args.custom_client_headers.clone()))
        .map(Arc::new)
        .map_err(|_| EdgeError::InvalidServerUrl(edge_args.upstream_url.clone()))?;
    if let Some(token_request) = edge_args.hmac_config.possible_token_request(
        unleash_client.backing_client.clone(),
        UnleashUrls::from_str(&edge_args.upstream_url)?.token_request_url,
    ) {
        let unleash_granted_tokens =
            unleash_edge_http_client::token_request::request_tokens(token_request).await?;
        if !edge_args.tokens.is_empty() {
            warn!(
                "Both tokens and hmac_config were configured. Overriding startup tokens with tokens obtained via hmac_config."
            );
        }
        edge_args.tokens = unleash_granted_tokens;
    }
    let startup_tokens = edge_args.tokens.clone();

    let (deferred_validation_tx, deferred_validation_rx) = if *SHOULD_DEFER_VALIDATION {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        (Some(tx), Some(rx))
    } else {
        (None, None)
    };

    let (
        (token_cache, features_cache, delta_cache_manager, engine_cache),
        token_validator,
        hydrator_type,
        persistence,
    ) = build_edge(
        &edge_args,
        client_meta_information.clone(),
        edge_instance_data.clone(),
        auth_headers.clone(),
        http_client.clone(),
        deferred_validation_tx,
    )
    .await?;

    let license_state = ApplicationLicenseState::new(
        match resolve_license(
            &unleash_client,
            persistence.clone(),
            &startup_tokens,
            &client_meta_information,
        )
        .await?
        {
            LicenseState::Valid => LicenseState::Valid,
            LicenseState::Expired => LicenseState::Expired,
            LicenseState::Invalid => Err(EdgeError::HeartbeatError(
                "License is invalid".into(),
                StatusCode::FORBIDDEN,
            ))?,
        },
    );

    let instance_data_sender: Arc<InstanceDataSending> = Arc::new(InstanceDataSending::from_args(
        args.clone(),
        &client_meta_information,
        http_client.clone(),
    )?);
    let metrics_cache = Arc::new(MetricsCache::default());

    let background_tasks = create_edge_mode_background_tasks(BackgroundTaskArgs {
        app_name: args.app_name.clone(),
        client_id: args.client_id.clone(),
        client_meta_information: client_meta_information.clone(),
        deferred_validation_rx,
        edge: edge_args.clone(),
        edge_instance_data: edge_instance_data.clone(),
        feature_cache: features_cache.clone(),
        instance_data_sender: instance_data_sender.clone(),
        instances_observed_for_app_context: instances_observed_for_app_context.clone(),
        metrics_cache_clone: metrics_cache.clone(),
        persistence: persistence.clone(),
        refresher: hydrator_type.clone(),
        startup_tokens: startup_tokens.clone(),
        token_cache: token_cache.clone(),
        unleash_client: unleash_client.clone(),
        validator: token_validator.clone(),
        license_state: license_state.clone(),
    });
    let shutdown_args = ShutdownTaskArgs {
        delta_cache_manager: delta_cache_manager.clone(),
        edge_instance_data: edge_instance_data.clone(),
        feature_cache: features_cache.clone(),
        instance_data_sender: instance_data_sender.clone(),
        instances_observed_for_app_context: instances_observed_for_app_context.clone(),
        metrics_cache: metrics_cache.clone(),
        persistence: persistence.clone(),
        startup_tokens,
        token_cache: token_cache.clone(),
        unleash_client: unleash_client.clone(),
    };
    let shutdown_tasks = create_shutdown_tasks(shutdown_args);

    let app_state = AppState {
        token_cache,
        features_cache,
        engine_cache,
        hydrator: Some(hydrator_type),
        token_validator: Arc::new(Some(token_validator.as_ref().clone())),
        metrics_cache,
        delta_cache_manager: Some(delta_cache_manager),
        edge_instance_data,
        connected_instances: instances_observed_for_app_context.clone(),
        deny_list: args.http.deny_list.unwrap_or_default(),
        allow_list: args.http.allow_list.unwrap_or_default(),
        auth_headers: auth_headers.clone(),
        connect_via: ConnectVia {
            app_name: args.app_name.clone(),
            instance_id: client_meta_information.instance_id.clone().to_string(),
        },
        license_state: license_state.clone(),
    };

    Ok((app_state, background_tasks, shutdown_tasks))
}

pub(crate) struct ShutdownTaskArgs {
    persistence: Option<Arc<dyn EdgePersistence>>,
    delta_cache_manager: Arc<DeltaCacheManager>,
    token_cache: Arc<TokenCache>,
    feature_cache: Arc<FeatureCache>,
    metrics_cache: Arc<MetricsCache>,
    startup_tokens: Vec<EdgeToken>,
    unleash_client: Arc<UnleashClient>,
    instance_data_sender: Arc<InstanceDataSending>,
    edge_instance_data: Arc<EdgeInstanceData>,
    instances_observed_for_app_context: Arc<RwLock<Vec<EdgeInstanceData>>>,
}
fn create_shutdown_tasks(
    ShutdownTaskArgs {
        persistence,
        delta_cache_manager,
        token_cache,
        feature_cache,
        metrics_cache,
        startup_tokens,
        unleash_client,
        instance_data_sender,
        edge_instance_data,
        instances_observed_for_app_context,
    }: ShutdownTaskArgs,
) -> Vec<BackgroundTask> {
    let mut tasks = vec![];

    if let Some(persistence) = persistence {
        tasks.push(create_once_off_persist(
            persistence,
            token_cache.clone(),
            feature_cache,
        ));
    }

    tasks.push(create_once_off_send_metrics(
        metrics_cache,
        unleash_client,
        startup_tokens,
    ));

    tasks.push(create_once_off_send_instance_data(
        instance_data_sender.clone(),
        edge_instance_data.clone(),
        instances_observed_for_app_context.clone(),
    ));

    tasks.push(create_terminate_sse_connections_task(
        delta_cache_manager.clone(),
    ));

    if let Some(otel) = OTEL_INIT.get() {
        tasks.push(shutdown_logging(otel.clone()));
    };

    tasks
}
pub(crate) struct BackgroundTaskArgs {
    app_name: String,
    client_id: Option<String>,
    client_meta_information: ClientMetaInformation,
    deferred_validation_rx: Option<tokio::sync::mpsc::UnboundedReceiver<String>>,
    edge: EdgeArgs,
    edge_instance_data: Arc<EdgeInstanceData>,
    feature_cache: Arc<FeatureCache>,
    instance_data_sender: Arc<InstanceDataSending>,
    instances_observed_for_app_context: Arc<RwLock<Vec<EdgeInstanceData>>>,
    metrics_cache_clone: Arc<MetricsCache>,
    persistence: Option<Arc<dyn EdgePersistence>>,
    refresher: HydratorType,
    startup_tokens: Vec<EdgeToken>,
    token_cache: Arc<TokenCache>,
    unleash_client: Arc<UnleashClient>,
    validator: Arc<TokenValidator>,
    license_state: ApplicationLicenseState,
}
fn create_edge_mode_background_tasks(
    BackgroundTaskArgs {
        app_name,
        client_id,
        client_meta_information,
        deferred_validation_rx,
        edge,
        edge_instance_data,
        feature_cache,
        instance_data_sender,
        instances_observed_for_app_context,
        metrics_cache_clone,
        startup_tokens,
        persistence,
        refresher,
        token_cache,
        unleash_client,
        validator,
        #[allow(unused_variables)] // license_state used in enterprise feature
        license_state,
    }: BackgroundTaskArgs,
) -> Vec<BackgroundTask> {
    #[allow(unused_variables)] // refresh_state_tx used in enterprise feature
    let (refresh_state_tx, refresh_state_rx) = channel(RefreshState::Running);

    let mut tasks: Vec<BackgroundTask> = vec![
        create_send_metrics_task(
            metrics_cache_clone.clone(),
            unleash_client.clone(),
            startup_tokens.clone(),
            edge.metrics_interval_seconds.try_into().unwrap(),
        ),
        create_revalidation_task(&validator, edge.token_revalidation_interval_seconds),
        create_revalidation_of_startup_tokens_task(
            &validator,
            edge.tokens.clone().into_iter().map(|t| t.token).collect(),
            refresher.clone(),
        ),
        create_send_instance_data_task(
            instance_data_sender.clone(),
            edge_instance_data.clone(),
            instances_observed_for_app_context.clone(),
        ),
        observe_tokens_in_background(
            edge_instance_data.app_name.clone(),
            edge_instance_data.identifier.clone(),
            validator.clone(),
        ),
    ];

    if let Some(url) = edge.clone().prometheus_remote_write_url {
        tasks.push(create_prometheus_write_task(PrometheusWriteTaskArgs {
            url,
            interval: edge.prometheus_push_interval,
            app_name,
            client_id,
            username: edge.prometheus_username.clone(),
            password: edge.prometheus_password.clone(),
        }));
    }

    let hydration_task = match &refresher {
        HydratorType::Streaming(delta_refresher) => create_stream_task(
            &edge,
            client_meta_information.clone(),
            delta_refresher.clone(),
            refresh_state_rx,
        ),
        HydratorType::Polling(feature_refresher) => {
            create_poll_task(feature_refresher.clone(), refresh_state_rx)
        }
    };
    tasks.push(hydration_task);

    if let Some(persistence) = persistence.clone() {
        tasks.push(create_persist_data_task(
            persistence.clone(),
            token_cache.clone(),
            feature_cache.clone(),
        ));
    } else {
        info!("No persistence configured, skipping persistence");
    }

    if let Some(rx) = deferred_validation_rx {
        tasks.push(create_deferred_validation_task(validator, rx));
    }

    #[cfg(feature = "enterprise")]
    {
        use unleash_edge_enterprise::create_enterprise_heartbeat_task;

        tasks.push(create_enterprise_heartbeat_task(
            unleash_client,
            startup_tokens
                .first()
                .cloned()
                .expect("Startup token is required for enterprise feature"),
            refresh_state_tx,
            client_meta_information.connection_id,
            license_state,
            persistence,
        ));
    }

    tasks
}

fn create_poll_task(
    feature_refresher: Arc<FeatureRefresher>,
    refresh_state_rx: Receiver<RefreshState>,
) -> Pin<Box<dyn Future<Output = ()> + Send>> {
    info!("Starting polling background task");
    Box::pin(async move {
        start_refresh_features_background_task(feature_refresher, refresh_state_rx).await;
    })
}

fn create_stream_task(
    edge: &EdgeArgs,
    client_meta_information: ClientMetaInformation,
    delta_refresher: Arc<DeltaRefresher>,
    refresh_state_rx: Receiver<RefreshState>,
) -> Pin<Box<dyn Future<Output = ()> + Send>> {
    let custom_headers = edge.custom_client_headers.clone();
    Box::pin(async move {
        let _ = start_streaming_delta_background_task(
            delta_refresher,
            client_meta_information,
            custom_headers,
            refresh_state_rx,
        )
        .await;
    })
}

struct LoadHydratorArgs<'a> {
    args: &'a EdgeArgs,
    unleash_client: Arc<UnleashClient>,
    feature_cache: Arc<FeatureCache>,
    delta_cache_manager: Arc<DeltaCacheManager>,
    engine_cache: Arc<EngineCache>,
    persistence: Option<Arc<dyn EdgePersistence>>,
    feature_config: &'a FeatureRefreshConfig,
    client_meta_information: &'a ClientMetaInformation,
    edge_instance_data: Arc<EdgeInstanceData>,
}

#[cfg(feature = "enterprise")]
fn load_hydrator(
    LoadHydratorArgs {
        args,
        unleash_client,
        feature_cache,
        delta_cache_manager,
        engine_cache,
        persistence,
        feature_config,
        client_meta_information,
        edge_instance_data,
    }: LoadHydratorArgs,
) -> HydratorType {
    if args.streaming {
        let delta_refresher = Arc::new(DeltaRefresher {
            unleash_client: unleash_client.clone(),
            tokens_to_refresh: Arc::new(Default::default()),
            delta_cache_manager: delta_cache_manager.clone(),
            features_cache: feature_cache.clone(),
            engine_cache: engine_cache.clone(),
            refresh_interval: Duration::seconds(args.features_refresh_interval_seconds as i64),
            persistence: persistence.clone(),
            streaming: true,
            client_meta_information: client_meta_information.clone(),
            edge_instance_data: edge_instance_data.clone(),
        });

        HydratorType::Streaming(delta_refresher)
    } else {
        let feature_refresher = Arc::new(FeatureRefresher::new(
            unleash_client,
            feature_cache.clone(),
            delta_cache_manager.clone(),
            engine_cache.clone(),
            persistence.clone(),
            feature_config.clone(),
            edge_instance_data.clone(),
        ));

        HydratorType::Polling(feature_refresher)
    }
}

#[cfg(not(feature = "enterprise"))]
fn load_hydrator(
    #[allow(unused_variables)] LoadHydratorArgs {
        args,
        unleash_client,
        feature_cache,
        delta_cache_manager,
        engine_cache,
        persistence,
        feature_config,
        client_meta_information,
        edge_instance_data,
    }: LoadHydratorArgs,
) -> HydratorType {
    let feature_refresher = Arc::new(FeatureRefresher::new(
        unleash_client,
        feature_cache.clone(),
        delta_cache_manager.clone(),
        engine_cache.clone(),
        persistence.clone(),
        feature_config.clone(),
        edge_instance_data.clone(),
    ));

    HydratorType::Polling(feature_refresher)
}

#[cfg(feature = "enterprise")]
pub async fn resolve_license(
    unleash_client: &UnleashClient,
    persistence: Option<Arc<dyn EdgePersistence>>,
    startup_tokens: &[EdgeToken],
    client_meta_information: &ClientMetaInformation,
) -> Result<LicenseState, EdgeError> {
    debug!("Starting enterprise license check");
    match unleash_client
        .send_heartbeat(
            startup_tokens.first().unwrap(),
            &client_meta_information.instance_id,
        )
        .await
    {
        Ok(license) => {
            if let Some(persistence) = persistence {
                let _ = persistence.save_license_state(&license).await;
            }
            Ok(license)
        }

        Err(_) => {
            if let Some(persistence) = persistence {
                persistence.load_license_state().await.map_err(|_| {
                    EdgeError::HeartbeatError(
                        "Could not load license from either persistence or API".into(),
                        StatusCode::SERVICE_UNAVAILABLE,
                    )
                })
            } else {
                Err(EdgeError::HeartbeatError(
                    "Could not reach upstream API and no cached license found".into(),
                    StatusCode::SERVICE_UNAVAILABLE,
                ))
            }
        }
    }
}

#[cfg(not(feature = "enterprise"))]
pub async fn resolve_license(
    _unleash_client: &UnleashClient,
    _persistence: Option<Arc<dyn EdgePersistence>>,
    _startup_tokens: &[EdgeToken],
    _client_meta_information: &ClientMetaInformation,
) -> Result<LicenseState, EdgeError> {
    debug!("Running non enterprise Edge, skipping license check");
    Ok(LicenseState::Valid)
}

#[cfg(test)]
mod tests {
    use crate::edge_builder::build_edge;
    use std::path::Path;
    use std::str::FromStr;
    use std::sync::Arc;
    use ulid::Ulid;
    use unleash_edge_cli::{AuthHeaders, EdgeArgs, HmacConfig};
    use unleash_edge_http_client::ClientMetaInformation;
    use unleash_edge_types::metrics::instance_data::{ApiKeyIdentity, EdgeInstanceData, Hosting};
    use unleash_edge_types::tokens::EdgeToken;

    #[tokio::test]
    #[cfg(feature = "enterprise")]
    async fn restores_revision_id_from_backup_if_present() {
        let backup_folder = Path::new("../../../examples/backup/sandbox");
        let edge_args = EdgeArgs {
            upstream_url: "http://localhost:3063".to_string(),
            backup_folder: Some(backup_folder.to_path_buf()),
            metrics_interval_seconds: 0,
            features_refresh_interval_seconds: 30,
            token_revalidation_interval_seconds: 30,
            tokens: vec![
                EdgeToken::from_str(
                    "default:development.f1339a9b0e67fd8dafe0a19a85809fb88262b2e74c213087c6b3b3a9",
                )
                .unwrap(),
            ],
            pretrusted_tokens: None,
            custom_client_headers: vec![],
            skip_ssl_verification: false,
            client_identity: None,
            upstream_certificate_file: None,
            upstream_request_timeout: 0,
            upstream_socket_timeout: 0,
            redis: None,
            s3: None,
            streaming: false,
            delta: false,
            consumption: false,
            client_keepalive_timeout: 0,
            prometheus_remote_write_url: None,
            prometheus_push_interval: 0,
            prometheus_username: None,
            prometheus_password: None,
            prometheus_user_id: None,
            hmac_config: HmacConfig::default(),
        };
        let client_meta_information = ClientMetaInformation {
            app_name: "test_app".to_string(),
            instance_id: Ulid::new(),
            connection_id: Ulid::new(),
        };
        let edge_instance_data = Arc::new(EdgeInstanceData::new(
            &client_meta_information.app_name,
            &client_meta_information.instance_id,
            Some(Hosting::SelfHosted),
        ));
        let http_client = reqwest::Client::new();
        let _ = build_edge(
            &edge_args,
            client_meta_information,
            edge_instance_data.clone(),
            AuthHeaders::default(),
            http_client,
            None,
        )
        .await
        .unwrap();
        assert_eq!(edge_instance_data.edge_api_key_revision_ids.len(), 1);
        let expected_key = ApiKeyIdentity {
            environment: "development".to_string(),
            projects: vec!["default".to_string()],
        };
        let revision_info = edge_instance_data
            .edge_api_key_revision_ids
            .get(&expected_key)
            .expect("revision id should be present for backed up key");
        assert_eq!(revision_info.revision_id, 80543)
    }
}
