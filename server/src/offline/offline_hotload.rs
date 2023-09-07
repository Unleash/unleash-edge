use std::{
    collections::HashMap,
    fs::File,
    io::{BufReader, Read},
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
    time::Duration,
};

use dashmap::DashMap;
use serde::Deserialize;
use unleash_types::client_features::{
    ClientFeature, ClientFeatures, Strategy, Variant, WeightType,
};
use unleash_yggdrasil::EngineState;

use crate::{error::EdgeError, types::EdgeToken};

pub async fn start_hotload_loop(
    bootstrap_path: Option<PathBuf>,
    features_cache: Arc<DashMap<std::string::String, ClientFeatures>>,
    engine_cache: Arc<DashMap<std::string::String, EngineState>>,
    known_tokens: Vec<String>,
) {
    loop {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(1)) => {
                let bootstrap = bootstrap_path.as_ref().map(|bootstrap_path|load_bootstrap(bootstrap_path));
                match bootstrap {
                    Some(Ok(bootstrap)) => {
                        let edge_tokens: Vec<EdgeToken> = known_tokens
                        .iter()
                        .map(|token| EdgeToken::from_str(token).unwrap_or_else(|_| EdgeToken::offline_token(token)))
                        .collect();

                        for edge_token in edge_tokens {
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

pub(crate) fn load_offline_engine_cache(
    edge_token: &EdgeToken,
    features_cache: Arc<DashMap<String, ClientFeatures>>,
    engine_cache: Arc<DashMap<String, EngineState>>,
    client_features: ClientFeatures,
) {
    features_cache.insert(
        crate::tokens::cache_key(edge_token),
        client_features.clone(),
    );
    let mut engine_state = EngineState::default();
    engine_state.take_state(client_features);
    engine_cache.insert(crate::tokens::cache_key(edge_token), engine_state);
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
                    parameters: None,
                    sort_order: None,
                    segments: None,
                    constraints: None,
                    variants: None,
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
    }
}

pub(crate) fn load_bootstrap(bootstrap_path: &Path) -> Result<ClientFeatures, EdgeError> {
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
