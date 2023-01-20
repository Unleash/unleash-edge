use crate::error::EdgeError;
use crate::types::FeaturesProvider;
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use unleash_types::client_features::ClientFeatures;

#[derive(Debug, Clone)]
pub struct OfflineProvider {
    pub features: ClientFeatures,
}

impl FeaturesProvider for OfflineProvider {
    fn get_client_features(&self) -> ClientFeatures {
        self.features.clone()
    }
}

impl OfflineProvider {
    pub fn instantiate_provider(
        bootstrap_file: Option<PathBuf>,
    ) -> Result<OfflineProvider, EdgeError> {
        if let Some(bootstrap) = bootstrap_file {
            let file = File::open(bootstrap.clone()).map_err(|_| EdgeError::NoFeaturesFile)?;
            let reader = BufReader::new(file);
            let client_features: ClientFeatures = serde_json::from_reader(reader).map_err(|e| {
                let path = format!("{}", bootstrap.clone().display());
                EdgeError::InvalidBackupFile(path, e.to_string())
            })?;
            Ok(OfflineProvider::new(client_features))
        } else {
            Err(EdgeError::NoFeaturesFile)
        }
    }
    pub fn new(features: ClientFeatures) -> Self {
        OfflineProvider { features }
    }
}
