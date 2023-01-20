use crate::error::EdgeError;
use crate::types::{EdgeProvider, EdgeToken, FeaturesProvider, TokenProvider};
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use unleash_types::client_features::ClientFeatures;

#[derive(Debug, Clone)]
pub struct OfflineProvider {
    pub features: ClientFeatures,
    pub valid_tokens: Vec<EdgeToken>,
}

impl FeaturesProvider for OfflineProvider {
    fn get_client_features(&self) -> ClientFeatures {
        self.features.clone()
    }
}

impl TokenProvider for OfflineProvider {
    fn get_known_tokens(&self) -> Vec<EdgeToken> {
        self.valid_tokens.clone()
    }

    fn secret_is_valid(&self, secret: &String) -> bool {
        self.valid_tokens.iter().any(|t| &t.secret == secret)
    }

    fn token_details(&self, secret: String) -> Option<EdgeToken> {
        self.valid_tokens
            .clone()
            .into_iter()
            .find(|t| t.secret == secret)
    }
}

impl EdgeProvider for OfflineProvider {}

impl OfflineProvider {
    pub fn instantiate_provider(
        bootstrap_file: Option<PathBuf>,
        valid_tokens: Vec<String>,
    ) -> Result<OfflineProvider, EdgeError> {
        if let Some(bootstrap) = bootstrap_file {
            let file = File::open(bootstrap.clone()).map_err(|_| EdgeError::NoFeaturesFile)?;
            let reader = BufReader::new(file);
            let client_features: ClientFeatures = serde_json::from_reader(reader).map_err(|e| {
                let path = format!("{}", bootstrap.clone().display());
                EdgeError::InvalidBackupFile(path, e.to_string())
            })?;
            Ok(OfflineProvider::new(client_features, valid_tokens))
        } else {
            Err(EdgeError::NoFeaturesFile)
        }
    }
    pub fn new(features: ClientFeatures, valid_tokens: Vec<String>) -> Self {
        OfflineProvider {
            features,
            valid_tokens: valid_tokens
                .into_iter()
                .map(|t| EdgeToken::try_from(t))
                .filter(|t| t.is_ok())
                .map(|t| t.unwrap())
                .collect(),
        }
    }
}
