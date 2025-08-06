use std::fs;
use std::io::BufReader;
use std::path::PathBuf;
use unleash_types::client_features::ClientFeatures;

pub mod feature_refresher;
pub mod token_validator;
pub mod delta_refresher;

pub fn features_from_disk(path: &str) -> ClientFeatures {
    let path = PathBuf::from(path);
    let file = fs::File::open(path).unwrap();
    let reader = BufReader::new(file);
    serde_json::from_reader(reader).unwrap()
}
