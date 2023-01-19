use crate::error::EdgeError;
use crate::types::FeaturesProvider;
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use tracing::info;
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
        info!("Instantiate offline provider");
        if let Some(bootstrap) = bootstrap_file {
            info!("Opening bootstrap file");
            let file = File::open(bootstrap.clone()).map_err(|_| EdgeError::NoFeaturesFile)?;
            info!("Opened");
            let reader = BufReader::new(file);
            info!("Buffered reader");
            let client_features: ClientFeatures =
                serde_json::from_reader(reader).map_err(|_| {
                    let path = format!("{}", bootstrap.clone().display());
                    EdgeError::InvalidBackupFile(path)
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
