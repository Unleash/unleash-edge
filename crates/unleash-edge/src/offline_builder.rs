use crate::CacheContainer;
use crate::edge_builder::build_caches;
use dashmap::DashMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::str::FromStr;
use std::sync::Arc;
use unleash_edge_appstate::AppState;
use unleash_edge_cli::{CliArgs, OfflineArgs};
use unleash_edge_feature_cache::FeatureCache;
use unleash_edge_http_client::instance_data::InstanceDataSending;
use unleash_edge_offline::hotload::{
    create_hotload_task, load_bootstrap, load_offline_engine_cache,
};
use unleash_edge_types::errors::EdgeError;
use unleash_edge_types::metrics::MetricsCache;
use unleash_edge_types::tokens::EdgeToken;
use unleash_edge_types::{BackgroundTask, EdgeResult, TokenType};
use unleash_types::client_features::ClientFeatures;
use unleash_yggdrasil::EngineState;

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

pub async fn build_offline_app_state(
    args: CliArgs,
    offline_args: OfflineArgs,
) -> EdgeResult<(AppState, Vec<BackgroundTask>, Vec<BackgroundTask>)> {
    let (token_cache, features_cache, _, engine_cache) = build_offline(offline_args.clone())?;
    let metrics_cache = Arc::new(MetricsCache::default());

    let instance_data_sender = Arc::new(InstanceDataSending::SendNothing);

    let app_state = AppState::builder()
        .with_token_cache(token_cache.clone())
        .with_features_cache(features_cache.clone())
        .with_engine_cache(engine_cache.clone())
        .with_metrics_cache(metrics_cache.clone())
        .with_deny_list(args.http.deny_list.unwrap_or_default())
        .with_allow_list(args.http.allow_list.unwrap_or_default())
        .with_instance_sending(instance_data_sender)
        .build();

    let background_tasks =
        create_offline_background_tasks(features_cache, engine_cache, offline_args);

    // offline mode explicitly has nothing to do on shutdown
    Ok((app_state, background_tasks, vec![]))
}

fn create_offline_background_tasks(
    features_cache: Arc<FeatureCache>,
    engine_cache: Arc<DashMap<String, EngineState>>,
    offline_args: OfflineArgs,
) -> Vec<BackgroundTask> {
    vec![create_hotload_task(
        features_cache,
        engine_cache,
        offline_args,
    )]
}
