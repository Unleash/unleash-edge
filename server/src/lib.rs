pub mod auth;
#[cfg(not(tarpaulin_include))]
pub mod builder;
#[cfg(not(tarpaulin_include))]
pub mod cli;
pub mod client_api;
pub mod edge_api;
#[cfg(not(tarpaulin_include))]
pub mod error;
pub mod frontend_api;
pub mod http;
pub mod internal_backstage;
pub mod metrics;
pub mod middleware;
#[cfg(not(tarpaulin_include))]
pub mod openapi;
pub mod persistence;
#[cfg(not(tarpaulin_include))]
pub mod prom_metrics;
#[cfg(not(tarpaulin_include))]
pub mod tls;
pub mod tokens;
pub mod types;
pub mod urls;

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::BufReader;
    use std::path::PathBuf;
    use unleash_types::client_features::ClientFeatures;

    pub fn features_from_disk(path: &str) -> ClientFeatures {
        let path = PathBuf::from(path);
        let file = fs::File::open(path).unwrap();
        let reader = BufReader::new(file);
        serde_json::from_reader(reader).unwrap()
    }
}
