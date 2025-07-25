use std::fs::File;
use std::io::{BufReader, Read};
use std::str::FromStr;
use std::sync::Arc;

use chrono::Duration;
use dashmap::DashMap;
use reqwest::Url;
use tracing::{debug, error, warn};
use unleash_types::client_features::ClientFeatures;
use unleash_yggdrasil::{EngineState, UpdateMessage};

use crate::cli::{AuthHeaders, RedisMode};
use crate::delta_cache_manager::DeltaCacheManager;
use crate::feature_cache::FeatureCache;
use crate::http::refresher::feature_refresher::{FeatureRefreshConfig, FeatureRefresherMode};
use crate::http::unleash_client::ClientMetaInformation;
use crate::offline::offline_hotload::{load_bootstrap, load_offline_engine_cache};
use crate::persistence::EdgePersistence;
use crate::persistence::file::FilePersister;
use crate::persistence::redis::RedisPersister;
#[cfg(feature = "s3-persistence")]
use crate::persistence::s3::s3_persister::S3Persister;
use crate::{
    auth::token_validator::TokenValidator,
    cli::{EdgeArgs, OfflineArgs},
    error::EdgeError,
    http::{refresher::feature_refresher::FeatureRefresher, unleash_client::UnleashClient},
    types::{EdgeResult, EdgeToken, TokenType},
};

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

fn build_caches() -> CacheContainer {
    let token_cache: DashMap<String, EdgeToken> = DashMap::default();
    let features_cache: DashMap<String, ClientFeatures> = DashMap::default();
    let delta_cache_manager = DeltaCacheManager::new();
    let engine_cache: DashMap<String, EngineState> = DashMap::default();
    (
        Arc::new(token_cache),
        Arc::new(FeatureCache::new(features_cache)),
        Arc::new(delta_cache_manager),
        Arc::new(engine_cache),
    )
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

pub(crate) fn build_offline_mode(
    client_features: ClientFeatures,
    tokens: Vec<String>,
    client_tokens: Vec<String>,
    frontend_tokens: Vec<String>,
) -> EdgeResult<CacheContainer> {
    let (token_cache, features_cache, _delta_cache_manager, engine_cache) = build_caches();

    let edge_tokens: Vec<EdgeToken> = tokens
        .iter()
        .map(|token| EdgeToken::from_str(token).unwrap_or_else(|_| EdgeToken::offline_token(token)))
        .collect();

    let edge_client_tokens: Vec<EdgeToken> = client_tokens
        .iter()
        .map(|token| EdgeToken::from_str(token).unwrap_or_else(|_| EdgeToken::offline_token(token)))
        .map(|mut token| {
            token.token_type = Some(TokenType::Client);
            token
        })
        .collect();
    let edge_frontend_tokens: Vec<EdgeToken> = frontend_tokens
        .iter()
        .map(|token| EdgeToken::from_str(token).unwrap_or_else(|_| EdgeToken::offline_token(token)))
        .map(|mut token| {
            token.token_type = Some(TokenType::Frontend);
            token
        })
        .collect();
    for edge_token in edge_tokens {
        token_cache.insert(edge_token.token.clone(), edge_token.clone());

        load_offline_engine_cache(
            &edge_token,
            features_cache.clone(),
            engine_cache.clone(),
            client_features.clone(),
        );
    }
    for client_token in edge_client_tokens {
        token_cache.insert(client_token.token.clone(), client_token.clone());
        load_offline_engine_cache(
            &client_token,
            features_cache.clone(),
            engine_cache.clone(),
            client_features.clone(),
        );
    }
    for frontend_token in edge_frontend_tokens {
        token_cache.insert(frontend_token.token.clone(), frontend_token.clone());
        load_offline_engine_cache(
            &frontend_token,
            features_cache.clone(),
            engine_cache.clone(),
            client_features.clone(),
        )
    }
    Ok((
        token_cache,
        features_cache,
        _delta_cache_manager,
        engine_cache,
    ))
}

pub fn build_offline(offline_args: OfflineArgs) -> EdgeResult<CacheContainer> {
    if offline_args.tokens.is_empty() && offline_args.client_tokens.is_empty() {
        return Err(EdgeError::NoTokens(
            "No tokens provided. Tokens must be specified when running in offline mode".into(),
        ));
    }

    if let Some(bootstrap) = offline_args.bootstrap_file {
        let file = File::open(bootstrap.clone()).map_err(|_| EdgeError::NoFeaturesFile)?;

        let mut reader = BufReader::new(file);
        let mut content = String::new();

        reader
            .read_to_string(&mut content)
            .map_err(|_| EdgeError::NoFeaturesFile)?;

        let client_features = load_bootstrap(&bootstrap)?;

        build_offline_mode(
            client_features,
            offline_args.tokens,
            offline_args.client_tokens,
            offline_args.frontend_tokens,
        )
    } else {
        Err(EdgeError::NoFeaturesFile)
    }
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

pub async fn build_edge(
    args: &EdgeArgs,
    client_meta_information: ClientMetaInformation,
    auth_headers: AuthHeaders,
    http_client: reqwest::Client,
) -> EdgeResult<EdgeInfo> {
    if !args.strict {
        if !args.dynamic {
            error!(
                "You should explicitly opt into either strict or dynamic behavior. Edge has defaulted to dynamic to preserve legacy behavior, however we recommend using strict from now on. Not explicitly opting into a behavior will return an error on startup in a future release"
            );
        }
        warn!(
            "Dynamic behavior has been deprecated and we plan to remove it in a future release. If you have a use case for it, please reach out to us"
        );
    }

    if args.strict && args.tokens.is_empty() {
        return Err(EdgeError::NoTokens(
            "No tokens provided. Tokens must be specified when running with strict behavior".into(),
        ));
    }

    let (token_cache, feature_cache, delta_cache, engine_cache) = build_caches();

    let persistence = get_data_source(args).await;

    let unleash_client = Url::parse(&args.upstream_url.clone())
        .map(|url| {
            UnleashClient::from_url(
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

    let token_validator = Arc::new(TokenValidator {
        token_cache: token_cache.clone(),
        unleash_client: unleash_client.clone(),
        persistence: persistence.clone(),
    });
    let refresher_mode = match (args.strict, args.streaming) {
        (_, true) => FeatureRefresherMode::Streaming,
        (true, _) => FeatureRefresherMode::Strict,
        _ => FeatureRefresherMode::Dynamic,
    };
    let delta_cache_manager = Arc::new(DeltaCacheManager::new());
    let feature_config = FeatureRefreshConfig::new(
        Duration::seconds(args.features_refresh_interval_seconds as i64),
        refresher_mode,
        client_meta_information,
        args.delta,
        args.delta_diff,
    );
    let feature_refresher = Arc::new(FeatureRefresher::new(
        unleash_client,
        feature_cache.clone(),
        delta_cache_manager.clone(),
        engine_cache.clone(),
        persistence.clone(),
        feature_config,
    ));
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

    if args.strict && token_cache.is_empty() {
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
    Ok((
        (
            token_cache,
            feature_cache,
            delta_cache_manager,
            engine_cache,
        ),
        Some(token_validator),
        Some(feature_refresher),
        persistence,
    ))
}

#[cfg(test)]
mod tests {
    use chrono::Duration;

    use crate::{
        builder::{build_edge, build_offline},
        cli::{AuthHeaders, EdgeArgs, OfflineArgs},
        http::unleash_client::{ClientMetaInformation, HttpClientArgs, new_reqwest_client},
    };

    #[test]
    fn should_fail_with_empty_tokens_when_offline_mode() {
        let args = OfflineArgs {
            bootstrap_file: None,
            tokens: vec![],
            reload_interval: Default::default(),
            client_tokens: vec![],
            frontend_tokens: vec![],
        };

        let result = build_offline(args);
        assert!(result.is_err());
        assert_eq!(
            result.err().unwrap().to_string(),
            "No tokens provided. Tokens must be specified when running in offline mode"
        );
    }

    #[tokio::test]
    async fn should_fail_with_empty_tokens_when_strict() {
        let args = EdgeArgs {
            strict: true,
            tokens: vec![],
            ..Default::default()
        };

        let client_meta_information = ClientMetaInformation {
            app_name: "test-app".into(),
            instance_id: "test-instance-id".into(),
            connection_id: "test-connection-id".into(),
        };

        let client = new_reqwest_client(HttpClientArgs::default()).unwrap();

        let result = build_edge(
            &args,
            client_meta_information,
            AuthHeaders::default(),
            client,
        )
        .await;
        assert!(result.is_err());
        assert_eq!(
            result.err().unwrap().to_string(),
            "No tokens provided. Tokens must be specified when running with strict behavior"
        );
    }
}
