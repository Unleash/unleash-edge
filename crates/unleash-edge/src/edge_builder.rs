use crate::{CacheContainer, EdgeInfo};
use chrono::Duration;
use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;
use tracing::{debug, error, info, warn};
use unleash_edge_auth::token_validator::TokenValidator;
use unleash_edge_cli::{AuthHeaders, EdgeArgs, RedisMode};
use unleash_edge_delta::cache_manager::DeltaCacheManager;
use unleash_edge_feature_cache::FeatureCache;
use unleash_edge_feature_refresh::{FeatureRefreshConfig, FeatureRefresher, FeatureRefresherMode};
use unleash_edge_http_client::{ClientMetaInformation, UnleashClient};
use unleash_edge_persistence::file::FilePersister;
use unleash_edge_persistence::redis::RedisPersister;
use unleash_edge_persistence::s3::s3_persister::S3Persister;
use unleash_edge_persistence::EdgePersistence;
use unleash_edge_types::errors::EdgeError;
use unleash_edge_types::tokens::EdgeToken;
use unleash_edge_types::{EdgeResult, EngineCache, TokenCache, TokenType};
use unleash_yggdrasil::{EngineState, UpdateMessage};
use url::Url;

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
    // TODO: do we need to hydrate from persistant storage for delta?
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
    auth_headers: AuthHeaders,
    http_client: reqwest::Client,
    tx: Option<UnboundedSender<String>>
) -> EdgeResult<EdgeInfo> {
    if args.tokens.is_empty() {
        return Err(EdgeError::NoTokens(            "No tokens provided. Tokens must be specified".into(),
        ));
    }
    let (token_cache, feature_cache, delta_cache, engine_cache) = build_caches();

    let persistence = get_data_source(args).await;

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

    let refresher_mode = if args.streaming {
        FeatureRefresherMode::Streaming
    } else {
        FeatureRefresherMode::Strict
    };
    let delta_cache_manager = Arc::new(DeltaCacheManager::new());
    let feature_config = FeatureRefreshConfig::new(
        Duration::seconds(args.features_refresh_interval_seconds as i64),
        refresher_mode,
        client_meta_information,
        args.delta,
        args.delta_diff,
    );
    let feature_refresher = FeatureRefresher::new(
        unleash_client,
        feature_cache.clone(),
        delta_cache_manager.clone(),
        engine_cache.clone(),
        persistence.clone(),
        feature_config,
    );
    let _ = token_validator.register_tokens(args.tokens.clone()).await;
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
        error!(
            "You started Edge in strict mode, but Edge was not able to validate any of the tokens configured at startup"
        );
        return Err(EdgeError::NoTokens("No valid tokens was provided on startup. At least one valid token must be specified at startup when running in Strict mode".into()));
    }
    for validated_token in token_cache
        .iter()
        .filter(|candidate| candidate.value().token_type == Some(TokenType::Client))
    {
        feature_refresher
            .register_token_for_refresh(validated_token.clone(), None)
            .await;
    }
    feature_refresher.hydrate_new_tokens().await;
    Ok((
        (
            token_cache,
            feature_cache,
            delta_cache_manager,
            engine_cache,
        ),
        Arc::new(Some(token_validator)),
        Arc::new(Some(feature_refresher)),
        persistence,
    ))
}