use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use serde::Deserialize;
use tracing::warn;
use unleash_types::client_features::{ClientFeature, ClientFeatures, Strategy, Variant, WeightType};
use unleash_yggdrasil::{EngineState, UpdateMessage};
use unleash_edge_cli::OfflineArgs;
use unleash_edge_feature_cache::FeatureCache;
use unleash_edge_types::EngineCache;
use unleash_edge_types::errors::EdgeError;
use unleash_edge_types::tokens::{cache_key, EdgeToken};

pub async fn start_hotload_loop(
    features_cache: Arc<FeatureCache>,
    engine_cache: Arc<EngineCache>,
    offline_args: OfflineArgs,
) {
    let mut known_tokens = offline_args.tokens;
    known_tokens.extend(offline_args.client_tokens);
    known_tokens.extend(offline_args.frontend_tokens);
    let bootstrap_path = offline_args.bootstrap_file;

    loop {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(offline_args.reload_interval)) => {
                let bootstrap = bootstrap_path.as_ref().map(|bootstrap_path|load_bootstrap(bootstrap_path));
                tracing::info!("Reloading bootstrap file");
                match bootstrap {
                    Some(Ok(bootstrap)) => {
                        tracing::info!("Found bootstrap file");
                        let edge_tokens: Vec<EdgeToken> = known_tokens
                        .iter()
                        .map(|token| EdgeToken::from_str(token).unwrap_or_else(|_| EdgeToken::offline_token(token)))
                        .collect();
                        tracing::info!("Edge tokens: {:?}", edge_tokens);
                        for edge_token in edge_tokens {
                            tracing::info!("Refreshing for {edge_token:?}");
                            load_offline_engine_cache(&edge_token, features_cache.clone(), engine_cache.clone(), bootstrap.clone());
                        }
                    },
                    Some(Err(e)) => {
                        tracing::error!("Error loading bootstrap file: {:?}", e);
                    }
                    None => {
                        tracing::debug!("No bootstrap file provided");
                    }
                };
            }
        }
    }
}

pub fn load_offline_engine_cache(
    edge_token: &EdgeToken,
    features_cache: Arc<FeatureCache>,
    engine_cache: Arc<EngineCache>,
    client_features: ClientFeatures,
) {
    features_cache.insert(
        cache_key(edge_token),
        client_features.clone(),
    );
    let mut engine = EngineState::default();
    let warnings = engine.take_state(UpdateMessage::FullResponse(client_features));
    engine_cache.insert(cache_key(edge_token), engine);
    if let Some(warnings) = warnings {
        warn!("The following toggle failed to compile and will be defaulted to off: {warnings:?}");
    }
}

#[derive(Deserialize)]
struct SimpleFeature {
    enabled: bool,
    variant: Option<String>,
}

fn make_simple_bootstrap(simple_bootstrap: HashMap<String, SimpleFeature>) -> ClientFeatures {
    let features = simple_bootstrap
        .iter()
        .map(|(feature_name, simple_feat)| {
            let variants = simple_feat.variant.as_ref().map(|variant_name| {
                vec![Variant {
                    name: variant_name.clone(),
                    weight: 1000,
                    weight_type: Some(WeightType::Fix),
                    stickiness: Some("default".into()),
                    payload: None,
                    overrides: None,
                }]
            });

            ClientFeature {
                name: feature_name.clone(),
                enabled: simple_feat.enabled,
                variants,
                strategies: Some(vec![Strategy {
                    name: "default".into(),
                    parameters: Some(HashMap::new()),
                    sort_order: None,
                    segments: None,
                    constraints: Some(vec![]),
                    variants: Some(vec![]),
                }]),
                project: Some("default".into()),
                ..Default::default()
            }
        })
        .collect();
    ClientFeatures {
        version: 2,
        features,
        segments: None,
        query: None,
        meta: None,
    }
}

pub fn load_bootstrap(bootstrap_path: &Path) -> Result<ClientFeatures, EdgeError> {
    let file = File::open(bootstrap_path).map_err(|_| EdgeError::NoFeaturesFile)?;

    let mut reader = BufReader::new(file);
    let mut content = String::new();

    reader
        .read_to_string(&mut content)
        .map_err(|_| EdgeError::NoFeaturesFile)?;

    parse_bootstrap(content).map_err(|e| {
        let path = format!("{}", bootstrap_path.to_path_buf().display());
        EdgeError::InvalidBackupFile(path, e.to_string())
    })
}

fn parse_bootstrap(content: String) -> Result<ClientFeatures, serde_json::Error> {
    let client_features: Result<ClientFeatures, serde_json::Error> =
        serde_json::from_str::<HashMap<String, SimpleFeature>>(&content)
            .map(make_simple_bootstrap)
            .or_else(|_| serde_json::from_str(&content));

    client_features
}

#[cfg(test)]
mod tests {
    use super::parse_bootstrap;

    #[test]
    fn loads_simple_bootstrap_format() {
        let simple_bootstrap = r#"
        {
            "feature1": {
                "enabled": true,
                "variant": "variant1"
            }
        }"#;
        parse_bootstrap(simple_bootstrap.to_string()).unwrap();
    }

    #[test]
    fn simple_bootstrap_parses_to_client_features_correctly() {
        let simple_bootstrap = r#"
        {
            "feature1": {
                "enabled": true,
                "variant": "variant1"
            }
        }"#;
        let client_features = parse_bootstrap(simple_bootstrap.to_string()).unwrap();
        assert_eq!(client_features.features.len(), 1);
        assert_eq!(client_features.features[0].name, "feature1");
        assert!(client_features.features[0].enabled);
        assert_eq!(
            client_features.features[0].variants.as_ref().unwrap()[0].name,
            "variant1"
        );
    }

    #[test]
    fn simple_bootstrap_does_not_require_variants() {
        let simple_bootstrap = r#"
        {
            "feature1": {
                "enabled": true
            }
        }"#;
        parse_bootstrap(simple_bootstrap.to_string()).unwrap();
    }

    #[test]
    fn falls_back_to_standard_unleash_format() {
        let simple_bootstrap = r#"
        {
            "version": 2,
            "features": [
              {
                "strategies": [
                  {
                    "name": "default",
                    "constraints": [],
                    "parameters": {}
                  }
                ],
                "impressionData": false,
                "enabled": true,
                "name": "custom.constraint",
                "description": "",
                "project": "default",
                "stale": false,
                "type": "release",
                "variants": []
              }
            ],
            "query": {
              "environment": "development",
              "inlineSegmentConstraints": true
            }
          }"#;
        parse_bootstrap(simple_bootstrap.to_string()).unwrap();
    }
}