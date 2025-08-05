use std::fs::File;
use std::io::{BufReader, Read};
use std::str::FromStr;
use unleash_types::client_features::ClientFeatures;
use unleash_edge_cli::OfflineArgs;
use unleash_edge_offline::hotload::{load_bootstrap, load_offline_engine_cache};
use unleash_edge_types::{EdgeResult, TokenType};
use unleash_edge_types::errors::EdgeError;
use unleash_edge_types::tokens::EdgeToken;
use crate::CacheContainer;
use crate::edge_builder::build_caches;

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