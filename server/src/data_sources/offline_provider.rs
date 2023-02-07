use crate::error::EdgeError;
use crate::types::{
    ClientFeaturesResponse, EdgeResult, EdgeSink, EdgeSource, EdgeToken, FeatureSink,
    FeaturesSource, TokenSink, TokenSource, TokenValidationStatus,
};
use actix_web::http::header::EntityTag;
use async_trait::async_trait;
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use unleash_types::client_features::ClientFeatures;

#[derive(Debug, Clone)]
pub struct OfflineProvider {
    pub features: ClientFeatures,
    pub valid_tokens: HashMap<String, EdgeToken>,
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
        Ok(self.valid_tokens.values().cloned().collect())
    }

    async fn get_token_validation_status(&self, secret: &str) -> EdgeResult<TokenValidationStatus> {
        Ok(if self.valid_tokens.contains_key(secret) {
            TokenValidationStatus::Validated
        } else {
            TokenValidationStatus::Invalid
        })
    }

    async fn token_details(&self, secret: String) -> EdgeResult<Option<EdgeToken>> {
        Ok(self.valid_tokens.get(&secret).cloned())
    }
    async fn get_valid_tokens(&self, secrets: Vec<String>) -> EdgeResult<Vec<EdgeToken>> {
        Ok(self
            .valid_tokens
            .clone()
            .into_iter()
            .filter(|(k, t)| t.status == TokenValidationStatus::Validated && secrets.contains(k))
            .map(|(_k, t)| t)
            .collect())
    }
}

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
    async fn fetch_features(&mut self, _token: &EdgeToken) -> EdgeResult<ClientFeaturesResponse> {
        Ok(ClientFeaturesResponse::NoUpdate(EntityTag::new_weak(
            "this_provider_does_not_support_refreshing_features".into(),
        )))
    }
}

#[async_trait]
impl TokenSink for OfflineProvider {
    async fn sink_tokens(&mut self, _token: Vec<EdgeToken>) -> EdgeResult<()> {
        todo!()
    }
}

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
                .map(|t| (t.token.clone(), t))
                .collect(),
        }
    }
}
