use crate::error::EdgeError;
use crate::types::{
    self, EdgeProvider, EdgeToken, FeaturesProvider, ProviderState, StateProvider, TokenProvider,
};
use actix_web::http::header::EntityTag;
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use unleash_types::client_features::ClientFeatures;

#[derive(Debug, Clone)]
pub struct OfflineProvider {
    pub features: ClientFeatures,
    pub features_hash: EntityTag,
    pub valid_tokens: Vec<EdgeToken>,
}

impl FeaturesProvider for OfflineProvider {
    fn get_client_features(&self, _: EdgeToken) -> ClientFeatures {
        self.features.clone()
    }
}

impl TokenProvider for OfflineProvider {
    fn get_known_tokens(&self) -> Vec<EdgeToken> {
        self.valid_tokens.clone()
    }

    fn secret_is_valid(&self, secret: &str) -> bool {
        self.valid_tokens.iter().any(|t| t.secret == secret)
    }

    fn token_details(&self, secret: String) -> Option<EdgeToken> {
        self.valid_tokens
            .clone()
            .into_iter()
            .find(|t| t.secret == secret)
    }
}

impl StateProvider for OfflineProvider {
    fn get_provider_state(&self, token: EdgeToken) -> Option<ProviderState> {
        match self.valid_tokens.contains(&token) {
            true => Some(ProviderState {
                features: self.features.clone(),
                token,
                hash: self.features_hash.clone(),
            }),
            false => None,
        }
    }
}

impl EdgeProvider for OfflineProvider {}

pub fn read_file(path: PathBuf) -> Result<BufReader<File>, EdgeError> {
    File::open(path)
        .map_err(|_| EdgeError::NoFeaturesFile)
        .map(BufReader::new)
}

impl OfflineProvider {
    pub fn instantiate_provider(
        bootstrap_file: Option<PathBuf>,
        valid_tokens: Vec<String>,
    ) -> Result<OfflineProvider, EdgeError> {
        if let Some(bootstrap) = bootstrap_file {
            let reader = read_file(bootstrap.clone())?;
            let client_features: ClientFeatures = serde_json::from_reader(reader).map_err(|e| {
                let path = format!("{}", bootstrap.clone().display());
                EdgeError::InvalidBackupFile(path, e.to_string())
            })?;
            Ok(OfflineProvider::new(client_features, valid_tokens))
        } else {
            Err(EdgeError::NoFeaturesFile)
        }
    }

    pub fn new(client_features: ClientFeatures, valid_tokens: Vec<String>) -> Self {
        let hash = types::calculate_hash(client_features.clone());
        let second_hash = types::calculate_hash(client_features.clone());
        assert_eq!(hash, second_hash);
        OfflineProvider {
            features: client_features,
            features_hash: EntityTag::new_weak(hash),
            valid_tokens: valid_tokens
                .into_iter()
                .map(EdgeToken::try_from)
                .filter_map(|t| t.ok())
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use actix_web::http::header::EntityTag;
    use test_case::test_case;
    use unleash_types::client_features::ClientFeatures;

    use super::{read_file, OfflineProvider};

    #[test_case("../examples/features.json".into(), "secret-123".into(), "XnJNYQuTw_PL91C1wkR58A".into() ; "Example features file and one key")]
    #[test_case("../examples/features2.json".into(), "secret-123,proxy-123".into(), "YbqhuJGV7mHeO1hVhzYNKg".into() ; "Different features file and two keys")]
    pub fn can_build_provider(path: PathBuf, keys: String, expected_hash: String) {
        let e_tag = EntityTag::new_weak(expected_hash);
        let k: Vec<String> = keys.split(',').map(|s| s.into()).collect();
        let provider = OfflineProvider::instantiate_provider(Some(path), k).unwrap();
        assert_eq!(provider.features_hash, e_tag);
    }

    #[test_case("../examples/features.json".into())]
    #[test_case("../examples/features2.json".into())]
    pub fn can_read_file_and_serde_is_idempotent(path: PathBuf) {
        let file_reader = read_file(path).unwrap();
        let client_features: ClientFeatures =
            serde_json::from_reader(file_reader).expect("Could not read json");
        let to_string = serde_json::to_string(&client_features).unwrap();
        let resered: String = serde_json::from_str::<ClientFeatures>(to_string.as_str())
            .and_then(|client_features| serde_json::to_string(&client_features))
            .expect("features");
        assert_eq!(to_string, resered);
    }
}
