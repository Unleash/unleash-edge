use s3::{creds::Credentials, Bucket, Region};
use std::collections::HashMap;

use async_trait::async_trait;
use unleash_types::client_features::ClientFeatures;

use crate::{
    error::EdgeError,
    types::{EdgeResult, EdgeToken},
};

use super::EdgePersistence;

pub const FEATURES_KEY: &str = "/unleash-features";
pub const TOKENS_KEY: &str = "/unleash-tokens";

pub struct S3Persister {
    bucket: Box<Bucket>,
}

impl S3Persister {
    pub fn new(bucket_name: &str, region: Region, creds: Credentials) -> EdgeResult<Self> {
        let bucket = Bucket::new(bucket_name, region, creds)
            .map_err(|err| EdgeError::PersistenceError(err.to_string()))?;
        Ok(S3Persister { bucket })
    }
}

#[async_trait]
impl EdgePersistence for S3Persister {
    async fn load_tokens(&self) -> EdgeResult<Vec<EdgeToken>> {
        self.bucket
            .get_object(TOKENS_KEY)
            .await
            .map_err(|err| EdgeError::PersistenceError(format!("Failed to load tokens: {}", err)))
            .map(|response| {
                serde_json::from_slice(&response.as_slice()).map_err(|_| {
                    EdgeError::PersistenceError("Failed to deserialize tokens".to_string())
                })
            })?
    }

    async fn save_tokens(&self, tokens: Vec<EdgeToken>) -> EdgeResult<()> {
        self.bucket
            .put_object(
                TOKENS_KEY,
                &serde_json::to_vec(&tokens).map_err(|_| {
                    EdgeError::PersistenceError("Failed to serialize features".to_string())
                })?,
            )
            .await
            .map(|_| ())
            .map_err(|err| EdgeError::PersistenceError(err.to_string()))
    }

    async fn load_features(&self) -> EdgeResult<HashMap<String, ClientFeatures>> {
        self.bucket
            .get_object(FEATURES_KEY)
            .await
            .map_err(|err| EdgeError::PersistenceError(format!("Failed to load features: {}", err)))
            .map(|response| {
                serde_json::from_slice(&response.as_slice()).map_err(|_| {
                    EdgeError::PersistenceError("Failed to deserialize features".to_string())
                })
            })?
    }

    async fn save_features(&self, features: Vec<(String, ClientFeatures)>) -> EdgeResult<()> {
        self.bucket
            .put_object(
                FEATURES_KEY,
                &serde_json::to_vec(&features).map_err(|_| {
                    EdgeError::PersistenceError("Failed to serialize features".to_string())
                })?,
            )
            .await
            .map(|_| ())
            .map_err(|err| EdgeError::PersistenceError(err.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use s3::creds::Credentials;
    use testcontainers::{runners::AsyncRunner, ImageExt};
    use testcontainers_modules::localstack::LocalStack;
    use unleash_types::client_features::ClientFeature;

    #[tokio::test]
    async fn test_s3_persister() {
        let localstack = LocalStack::default()
            .with_env_var("SERVICES", "s3")
            .start()
            .await
            .expect("Failed to start localstack");

        let bucket_name = "test-bucket";
        let local_stack_ip = localstack.get_host().await.expect("Could not get host");
        let local_stack_port = localstack.get_host_port_ipv4(4566).await.expect("Could not get port");
        let region = Region::Custom {
            region: "us-east-1".to_string(),
            endpoint: format!("http://{}:{}", local_stack_ip.to_string(),  local_stack_port),
        };

        let client_features_one = ClientFeatures {
            version: 2,
            features: vec![
                ClientFeature {
                    name: "feature1".into(),
                    ..ClientFeature::default()
                },
                ClientFeature {
                    name: "feature2".into(),
                    ..ClientFeature::default()
                },
            ],
            segments: None,
            query: None,
        };

        //hopefully we don't care, this should just work with localstack
        let creds =
            Credentials::from_sts("test", "test", "test").expect("Cannot create creds for test");
        let persister =
            S3Persister::new(bucket_name, region, creds).expect("Can't create persister");

        let tokens = vec![EdgeToken::from_str("*.default:abcdedfu").unwrap()];
        persister
            .save_tokens(tokens.clone())
            .await
            .expect("Failed to save tokens");

        let loaded_tokens = persister
            .load_tokens()
            .await
            .expect("Failed to load tokens");
        assert_eq!(tokens, loaded_tokens);

        let features = vec![("test".to_string(), client_features_one.clone())];
        persister.save_features(features.clone()).await.expect("Failed to save features");

        let loaded_features = persister.load_features().await.expect("Failed to load features");
        assert_eq!(
            features.into_iter().collect::<HashMap<_, _>>(),
            loaded_features
        );
    }
}
