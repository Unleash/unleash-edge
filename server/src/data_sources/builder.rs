use crate::auth::token_validator::TokenValidator;
use crate::http::unleash_client::UnleashClient;
use chrono::Duration;
use dashmap::DashMap;
use reqwest::Url;
use std::fs::File;
use std::sync::Arc;
use std::{io::BufReader, str::FromStr};

use crate::{
    cli::{CliArgs, EdgeArgs, EdgeMode, OfflineArgs},
    error::EdgeError,
    http::feature_refresher::FeatureRefresher,
    types::{EdgeResult, EdgeToken},
};
use unleash_types::client_features::ClientFeatures;
use unleash_yggdrasil::EngineState;

type CacheContainer = (
    Arc<DashMap<String, EdgeToken>>,
    Arc<DashMap<String, ClientFeatures>>,
    Arc<DashMap<String, EngineState>>,
);
type EdgeInfo = (
    CacheContainer,
    Option<Arc<TokenValidator>>,
    Option<Arc<FeatureRefresher>>,
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
        features_cache.insert(
            crate::tokens::cache_key(edge_token.clone()),
            client_features.clone(),
        );
        let mut engine_state = EngineState::default();
        engine_state.take_state(client_features.clone());
        engine_cache.insert(crate::tokens::cache_key(edge_token.clone()), engine_state);
    }
    Ok((token_cache, features_cache, engine_cache))
}

fn build_offline(offline_args: OfflineArgs) -> EdgeResult<CacheContainer> {
    if let Some(bootstrap) = offline_args.bootstrap_file {
        let file = File::open(bootstrap.clone()).map_err(|_| EdgeError::NoFeaturesFile)?;
        let reader = BufReader::new(file);
        let client_features: ClientFeatures = serde_json::from_reader(reader).map_err(|e| {
            let path = format!("{}", bootstrap.clone().display());
            EdgeError::InvalidBackupFile(path, e.to_string())
        })?;
        build_offline_mode(client_features, offline_args.tokens)
    } else {
        Err(EdgeError::NoFeaturesFile)
    }
}

fn build_edge(args: EdgeArgs) -> EdgeResult<EdgeInfo> {
    let (token_cache, feature_cache, engine_cache) = build_caches();

    let unleash_client = Url::parse(&args.upstream_url)
        .map(UnleashClient::from_url)
        .map(Arc::new)
        .map_err(|_| EdgeError::InvalidServerUrl(args.upstream_url))?;
    let token_validator = Arc::new(TokenValidator {
        token_cache: token_cache.clone(),
        unleash_client: unleash_client.clone(),
    });
    let feature_refresher = Arc::new(FeatureRefresher::new(
        unleash_client,
        feature_cache.clone(),
        engine_cache.clone(),
        Duration::seconds(args.features_refresh_interval_seconds),
    ));
    Ok((
        (token_cache, feature_cache, engine_cache),
        Some(token_validator),
        Some(feature_refresher),
    ))
}

pub async fn build_caches_and_refreshers(args: CliArgs) -> EdgeResult<EdgeInfo> {
    match args.mode {
        EdgeMode::Offline(offline_args) => {
            build_offline(offline_args).map(|cache| (cache, None, None))
        }
        EdgeMode::Edge(edge_args) => build_edge(edge_args),
    }
}
