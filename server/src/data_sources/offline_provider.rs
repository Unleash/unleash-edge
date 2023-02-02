use crate::error::EdgeError;
use crate::types::{EdgeProvider, EdgeResult, EdgeToken, FeaturesProvider, TokenProvider};
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
    fn get_client_features(&self, _: &EdgeToken) -> Result<ClientFeatures, EdgeError> {
        Ok(self.features.clone())
    }
}

impl TokenProvider for OfflineProvider {
    fn get_known_tokens(&self) -> EdgeResult<Vec<EdgeToken>> {
        Ok(self.valid_tokens.clone())
    }

    fn secret_is_valid(&self, secret: &str) -> EdgeResult<bool> {
        Ok(self.valid_tokens.iter().any(|t| t.token == secret))
    }

    fn token_details(&self, secret: String) -> EdgeResult<Option<EdgeToken>> {
        Ok(self
            .valid_tokens
            .clone()
            .into_iter()
            .find(|t| t.token == secret))
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
                .map(EdgeToken::try_from)
                .filter_map(|t| t.ok())
                .collect(),
        }
    }
}
