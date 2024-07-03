use std::fs::File;
use std::io::{BufReader, Read};
use std::str::FromStr;
use std::sync::Arc;

use chrono::Duration;
use dashmap::DashMap;
use reqwest::Url;
use tracing::{debug, warn};
use unleash_types::client_features::ClientFeatures;
use unleash_yggdrasil::EngineState;

use crate::cli::RedisMode;
use crate::offline::offline_hotload::{load_bootstrap, load_offline_engine_cache};
use crate::persistence::file::FilePersister;
use crate::persistence::redis::RedisPersister;
use crate::persistence::EdgePersistence;
use crate::{
    auth::token_validator::TokenValidator,
    cli::{CliArgs, EdgeArgs, EdgeMode, OfflineArgs},
    error::EdgeError,
    http::{feature_refresher::FeatureRefresher, unleash_client::UnleashClient},
    types::{EdgeResult, EdgeToken, TokenType},
};

type CacheContainer = (
    Arc<DashMap<String, EdgeToken>>,
    Arc<DashMap<String, ClientFeatures>>,
    Arc<DashMap<String, EngineState>>,
);
type EdgeInfo = (
    CacheContainer,
    Option<Arc<TokenValidator>>,
    Option<Arc<FeatureRefresher>>,
    Option<Arc<dyn EdgePersistence>>,
);

fn build_caches() -> CacheContainer {
    let token_cache: DashMap<String, EdgeToken> = DashMap::default();
    let features_cache: DashMap<String, ClientFeatures> = DashMap::default();
    let engine_cache: DashMap<String, EngineState> = DashMap::default();
    (
        Arc::new(token_cache),
        Arc::new(features_cache),
        Arc::new(engine_cache),
    )
}

async fn hydrate_from_persistent_storage(cache: CacheContainer, storage: Arc<dyn EdgePersistence>) {
    let (token_cache, features_cache, engine_cache) = cache;
    let tokens = storage.load_tokens().await.unwrap_or_else(|error| {
        warn!("Failed to load tokens from cache {error:?}");
        vec![]
    });
    let features = storage.load_features().await.unwrap_or_else(|error| {
        warn!("Failed to load features from cache {error:?}");
        Default::default()
    });
    for token in tokens {
        tracing::debug!("Hydrating tokens {token:?}");
        token_cache.insert(token.token.clone(), token);
    }

    for (key, features) in features {
        tracing::debug!("Hydrating features for {key:?}");
        features_cache.insert(key.clone(), features.clone());
        let mut engine_state = EngineState::default();

        let warnings = engine_state.take_state(features);
        if let Some(warnings) = warnings {
            warn!("Failed to hydrate features for {key:?}: {warnings:?}");
        }
        engine_cache.insert(key.clone(), engine_state);
    }
}

pub(crate) fn build_offline_mode(
    client_features: ClientFeatures,
    tokens: Vec<String>,
) -> EdgeResult<CacheContainer> {
    let (token_cache, features_cache, engine_cache) = build_caches();

    let edge_tokens: Vec<EdgeToken> = tokens
        .iter()
        .map(|token| EdgeToken::from_str(token).unwrap_or_else(|_| EdgeToken::offline_token(token)))
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
    Ok((token_cache, features_cache, engine_cache))
}

fn build_offline(offline_args: OfflineArgs) -> EdgeResult<CacheContainer> {
    if offline_args.tokens.is_empty() {
        return Err(EdgeError::NoTokens("No tokens provided. Tokens must be specified when running in offline mode".into()));
    }

    if let Some(bootstrap) = offline_args.bootstrap_file {
        let file = File::open(bootstrap.clone()).map_err(|_| EdgeError::NoFeaturesFile)?;

        let mut reader = BufReader::new(file);
        let mut content = String::new();

        reader
            .read_to_string(&mut content)
            .map_err(|_| EdgeError::NoFeaturesFile)?;

        let client_features = load_bootstrap(&bootstrap)?;

        build_offline_mode(client_features, offline_args.tokens)
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
            RedisMode::Single => redis_args
                .to_url()
                .map(|url| RedisPersister::new(&url).expect("Failed to connect to redis")),
            RedisMode::Cluster => redis_args.redis_url.map(|urls| {
                RedisPersister::new_with_cluster(urls).expect("Failed to connect to redis cluster")
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

    if let Some(backup_folder) = args.backup_folder.clone() {
        debug!("Configuring file persistence {backup_folder:?}");
        let backup_client = FilePersister::new(&backup_folder);
        return Some(Arc::new(backup_client));
    }

    None
}

async fn build_edge(args: &EdgeArgs) -> EdgeResult<EdgeInfo> {
    if !args.open && args.tokens.is_empty() {
        return Err(EdgeError::NoTokens("No tokens provided. Tokens must be specified when running in closed mode".into()));
    }

    let (token_cache, feature_cache, engine_cache) = build_caches();

    let persistence = get_data_source(args).await;

    let unleash_client = Url::parse(&args.upstream_url.clone())
        .map(|url| {
            UnleashClient::from_url(
                url,
                args.skip_ssl_verification,
                args.client_identity.clone(),
                args.upstream_certificate_file.clone(),
                Duration::seconds(args.upstream_request_timeout),
                Duration::seconds(args.upstream_socket_timeout),
                args.token_header.token_header.clone(),
            )
        })
        .map(|c| c.with_custom_client_headers(args.custom_client_headers.clone()))
        .map(Arc::new)
        .map_err(|_| EdgeError::InvalidServerUrl(args.upstream_url.clone()))?;

    let token_validator = Arc::new(TokenValidator {
        token_cache: token_cache.clone(),
        unleash_client: unleash_client.clone(),
        persistence: persistence.clone(),
    });

    let feature_refresher = Arc::new(FeatureRefresher::new(
        unleash_client,
        feature_cache.clone(),
        engine_cache.clone(),
        Duration::seconds(args.features_refresh_interval_seconds.try_into().unwrap()),
        persistence.clone(),
        args.open,
    ));
    let _ = token_validator.register_tokens(args.tokens.clone()).await;

    if let Some(persistence) = persistence.clone() {
        hydrate_from_persistent_storage(
            (
                token_cache.clone(),
                feature_cache.clone(),
                engine_cache.clone(),
            ),
            persistence,
        )
        .await;
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
        (token_cache, feature_cache, engine_cache),
        Some(token_validator),
        Some(feature_refresher),
        persistence,
    ))
}

pub async fn build_caches_and_refreshers(args: CliArgs) -> EdgeResult<EdgeInfo> {
    match args.mode {
        EdgeMode::Offline(offline_args) => {
            build_offline(offline_args).map(|cache| (cache, None, None, None))
        }
        EdgeMode::Edge(edge_args) => build_edge(&edge_args).await,
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use crate::{builder::{build_edge, build_offline}, cli::{EdgeArgs, OfflineArgs, TokenHeader}};

    #[test]
    fn should_fail_with_empty_tokens_when_offline_mode() {
        let args = OfflineArgs {
            bootstrap_file: None,
            tokens: vec![],
            reload_interval: Default::default()
        };

        let result = build_offline(args);
        assert!(result.is_err());
        assert_eq!(result
          .err()
          .unwrap()
          .to_string(), "No tokens provided. Tokens must be specified when running in offline mode");
    }

    #[tokio::test]
    async fn should_fail_with_empty_tokens_when_closed_mode() {
        let args = EdgeArgs {
            upstream_url: Default::default(),
            backup_folder: None,
            metrics_interval_seconds: Default::default(),
            features_refresh_interval_seconds: Default::default(),
            open: false,
            tokens: vec![],
            redis: None,
            client_identity: Default::default(),
            skip_ssl_verification: false,
            upstream_request_timeout: Default::default(),
            upstream_socket_timeout: Default::default(),
            custom_client_headers: Default::default(),
            token_header: TokenHeader { token_header: "Authorization".into() },
            upstream_certificate_file: Default::default(),
            token_revalidation_interval_seconds: Default::default(),
        };

        let result = build_edge(&args).await;
        assert!(result.is_err());
        assert_eq!(result
          .err()
          .unwrap()
          .to_string(), "No tokens provided. Tokens must be specified when running in closed mode");
    }
  }