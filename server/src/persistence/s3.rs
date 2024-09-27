use std::collections::HashMap;

use async_trait::async_trait;
use aws_config::SdkConfig;
use unleash_types::client_features::ClientFeatures;

use super::EdgePersistence;
use crate::{
    error::EdgeError,
    types::{EdgeResult, EdgeToken},
};
use aws_sdk_s3::{
    primitives::{ByteStream, SdkBody},
    Client, Config, Error,
};

pub const FEATURES_KEY: &str = "/unleash-features.json";
pub const TOKENS_KEY: &str = "/unleash-tokens.json";

pub struct S3Persister {
    client: Client,
    bucket: String,
}

impl S3Persister {
    pub fn new_with_config(bucket_name: &str, config: &SdkConfig) -> Self {
        let client = Client::new(config);
        Self {
            client,
            bucket: bucket_name.to_string(),
        }
    }
    pub async fn new_from_env(bucket_name: &str) -> Self {
        let shared_config = aws_config::load_from_env().await;
        let client = Client::new(&shared_config);
        Self {
            client,
            bucket: bucket_name.to_string(),
        }
    }
}

#[async_trait]
impl EdgePersistence for S3Persister {
    async fn load_tokens(&self) -> EdgeResult<Vec<EdgeToken>> {
        let response = self
            .client
            .get_object()
            .bucket(self.bucket.clone())
            .key(TOKENS_KEY)
            .response_content_type("application/json")
            .send()
            .await
            .map_err(|err| {
                EdgeError::PersistenceError(format!("Failed to load tokens: {}", err))
            })?;
        let data = response.body.collect().await.expect("Failed data");
        serde_json::from_slice(&data.to_vec())
            .map_err(|_| EdgeError::PersistenceError("Failed to deserialize tokens".to_string()))
    }

    async fn save_tokens(&self, tokens: Vec<EdgeToken>) -> EdgeResult<()> {
        let body_data = serde_json::to_vec(&tokens)
            .map_err(|_| EdgeError::PersistenceError("Failed to serialize tokens".to_string()))?;
        let byte_stream = ByteStream::new(SdkBody::from(body_data));
        self.client
            .put_object()
            .bucket(self.bucket.clone())
            .key(TOKENS_KEY)
            .body(byte_stream)
            .send()
            .await
            .map(|_| ())
            .map_err(|err| EdgeError::PersistenceError(err.to_string()))
    }

    async fn load_features(&self) -> EdgeResult<HashMap<String, ClientFeatures>> {
        let response = self
            .client
            .get_object()
            .bucket(self.bucket.clone())
            .key(FEATURES_KEY)
            .response_content_type("application/json")
            .send()
            .await
            .map_err(|err| {
                EdgeError::PersistenceError(format!("Failed to load features: {}", err))
            })?;
        let data = response.body.collect().await.expect("Failed data");
        serde_json::from_slice(&data.to_vec())
            .map_err(|_| EdgeError::PersistenceError("Failed to deserialize features".to_string()))
    }

    async fn save_features(&self, features: Vec<(String, ClientFeatures)>) -> EdgeResult<()> {
        let body_data = serde_json::to_vec(&features)
            .map_err(|_| EdgeError::PersistenceError("Failed to serialize tokens".to_string()))?;
        let byte_stream = ByteStream::new(SdkBody::from(body_data));
        self.client
            .put_object()
            .bucket(self.bucket.clone())
            .key(FEATURES_KEY)
            .body(byte_stream)
            .send()
            .await
            .map(|_| ())
            .map_err(|err| EdgeError::PersistenceError(err.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use aws_config::Region;
    use aws_config::SdkConfig;
    use aws_sdk_s3::config::SharedCredentialsProvider;
    use aws_sdk_s3::{config::Credentials, Config};
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
        let local_stack_port = localstack
            .get_host_port_ipv4(4566)
            .await
            .expect("Could not get port");

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
        let config = SdkConfig::builder()
            .region(Region::new("us-east-1"))
            .endpoint_url(format!("http://{}:{}", local_stack_ip, local_stack_port))
            .credentials_provider(SharedCredentialsProvider::new(Credentials::for_tests()))
            .build();

        //hopefully we don't care, this should just work with localstack
        let persister = S3Persister::new_with_config(bucket_name, &config);

        let tokens = vec![EdgeToken::from_str("eg:development.secret321").unwrap()];
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
        persister
            .save_features(features.clone())
            .await
            .expect("Failed to save features");

        let loaded_features = persister
            .load_features()
            .await
            .expect("Failed to load features");
        assert_eq!(
            features.into_iter().collect::<HashMap<_, _>>(),
            loaded_features
        );
    }
}
