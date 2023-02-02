use crate::error::EdgeError;
use crate::types::{
    EdgeProvider, EdgeResult, EdgeSink, EdgeSource, EdgeToken, FeatureSink, FeaturesSource,
    TokenSink, TokenSource,
};
use async_trait::async_trait;
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc::Sender;
use unleash_types::client_features::ClientFeatures;

#[derive(Debug, Clone)]
pub struct OfflineProvider {
    pub features: ClientFeatures,
    pub valid_tokens: Vec<EdgeToken>,
}

#[async_trait]
impl FeaturesSource for OfflineProvider {
    async fn get_client_features(&self, _: &EdgeToken) -> Result<ClientFeatures, EdgeError> {
        Ok(self.features.clone())
    }
}

#[async_trait]
impl TokenSource for OfflineProvider {
    async fn get_known_tokens(&self) -> EdgeResult<Vec<EdgeToken>> {
        Ok(self.valid_tokens.clone())
    }

    async fn secret_is_valid(&self, secret: &str, _: Arc<Sender<EdgeToken>>) -> EdgeResult<bool> {
        Ok(self.valid_tokens.iter().any(|t| t.token == secret))
    }

    async fn token_details(&self, secret: String) -> EdgeResult<Option<EdgeToken>> {
        Ok(self
            .valid_tokens
            .clone()
            .into_iter()
            .find(|t| t.token == secret))
    }
}

impl EdgeProvider for OfflineProvider {}
impl EdgeSource for OfflineProvider {}
impl EdgeSink for OfflineProvider {}

#[async_trait]
impl FeatureSink for OfflineProvider {
    async fn sink_features(
        &mut self,
        _token: &EdgeToken,
        _features: ClientFeatures,
    ) -> EdgeResult<()> {
        todo!()
    }

    async fn sink_tokens(&mut self, _token: Vec<EdgeToken>) -> EdgeResult<()> {
        todo!()
    }
}
impl TokenSink for OfflineProvider {}

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
