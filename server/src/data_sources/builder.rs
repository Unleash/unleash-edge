use std::{io::BufReader, str::FromStr};

use chrono::Duration;
use dashmap::DashMap;
use std::fs::File;

use crate::{
    cli::{CliArgs, EdgeMode, OfflineArgs},
    error::EdgeError,
    types::{EdgeResult, EdgeToken, FeatureRefresher},
};
use unleash_types::client_features::ClientFeatures;
use unleash_yggdrasil::EngineState;

type CacheContainer = (
    DashMap<String, EdgeToken>,
    DashMap<String, ClientFeatures>,
    DashMap<String, EngineState>,
);
type EdgeInfo = (CacheContainer, Option<FeatureRefresher>);

pub(crate) fn build_offline_mode(
    client_features: ClientFeatures,
    tokens: Vec<String>,
) -> EdgeResult<CacheContainer> {
    let token_cache: DashMap<String, EdgeToken> = DashMap::default();
    let features_cache: DashMap<String, ClientFeatures> = DashMap::default();
    let engine_cache: DashMap<String, EngineState> = DashMap::default();

    let edge_tokens: Vec<EdgeToken> = tokens
        .iter()
        .map(|token| {
            EdgeToken::from_str(&token).unwrap_or_else(|_| EdgeToken::offline_token(token))
        })
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

fn build_memory(features_refresh_interval_seconds: Duration) -> EdgeResult<EdgeInfo> {
    todo!()
}

pub async fn build_caches_and_refreshers(args: CliArgs) -> EdgeResult<EdgeInfo> {
    match args.mode {
        EdgeMode::Offline(offline_args) => build_offline(offline_args).map(|cache| (cache, None)),
        EdgeMode::Edge(edge_args) => {
            todo!()
        }
    }
}
