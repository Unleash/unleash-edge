#![cfg(feature = "s3-persistence")]
pub mod s3_persister {

    use std::collections::HashMap;

    use async_trait::async_trait;
    use unleash_types::client_features::ClientFeatures;

    use aws_sdk_s3::{
        self as s3,
        primitives::{ByteStream, SdkBody},
    };
    use unleash_edge_types::EdgeResult;
    use unleash_edge_types::errors::EdgeError;
    use unleash_edge_types::tokens::EdgeToken;
    use crate::EdgePersistence;

    pub const FEATURES_KEY: &str = "/unleash-features.json";
    pub const TOKENS_KEY: &str = "/unleash-tokens.json";

    pub struct S3Persister {
        client: s3::Client,
        bucket: String,
    }

    impl S3Persister {
        pub fn new_with_config(bucket_name: &str, config: s3::config::Config) -> Self {
            let client = s3::Client::from_conf(config);
            Self {
                client,
                bucket: bucket_name.to_string(),
            }
        }
        pub async fn new_from_env(bucket_name: &str) -> Self {
            let shared_config = aws_config::load_from_env().await;
            let client = s3::Client::new(&shared_config);
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
                .await.map_err(|_| EdgeError::PersistenceError("Failed to GET tokens".to_string()))?;
            let data = response.body.collect().await.expect("Failed data");
            serde_json::from_slice(&data.to_vec()).map_err(|_| {
                EdgeError::PersistenceError("Failed to deserialize tokens".to_string())
            })
        }

        async fn save_tokens(&self, tokens: Vec<EdgeToken>) -> EdgeResult<()> {
            let body_data = serde_json::to_vec(&tokens)
                .map_err(|_| EdgeError::PersistenceError("Failed to serialize tokens".to_string()))
                .map(SdkBody::from)?;
            let byte_stream = aws_sdk_s3::primitives::ByteStream::new(body_data);
            self.client
                .put_object()
                .bucket(self.bucket.clone())
                .key(TOKENS_KEY)
                .body(byte_stream)
                .send()
                .await
                .map(|_| ())
                .map_err(|_err| EdgeError::PersistenceError("Failed to save tokens".to_string()))
        }

        async fn load_features(&self) -> EdgeResult<HashMap<String, ClientFeatures>> {
            let query = self
                .client
                .get_object()
                .bucket(self.bucket.clone())
                .key(FEATURES_KEY)
                .response_content_type("application/json")
                .send()
                .await
                .map_err(|err| {
                    if err.to_string().contains("NoSuchKey") {
                        return EdgeError::PersistenceError("No features found".to_string());
                    }
                    EdgeError::PersistenceError("Failed to load features".to_string())
                });
            match query {
                Ok(response) => {
                    let data = response.body.collect().await.expect("Failed data");
                    let deser: Vec<(String, ClientFeatures)> =
                        serde_json::from_slice(&data.to_vec()).map_err(|_| {
                            EdgeError::PersistenceError(
                                "Failed to deserialize features".to_string(),
                            )
                        })?;
                    Ok(deser
                        .iter()
                        .cloned()
                        .collect::<HashMap<String, ClientFeatures>>())
                }
                Err(_e) => Ok(HashMap::new()),
            }
        }

        async fn save_features(&self, features: Vec<(String, ClientFeatures)>) -> EdgeResult<()> {
            let body_data = serde_json::to_vec(&features).map_err(|_| {
                EdgeError::PersistenceError("Failed to serialize features".to_string())
            })?;
            let byte_stream = ByteStream::new(SdkBody::from(body_data));
            match self
                .client
                .put_object()
                .bucket(self.bucket.clone())
                .key(FEATURES_KEY)
                .body(byte_stream)
                .send()
                .await
            {
                Ok(_) => Ok(()),
                Err(_s3_err) => Err(EdgeError::PersistenceError(
                    "Failed to save features".to_string(),
                )),
            }
        }
    }
}
