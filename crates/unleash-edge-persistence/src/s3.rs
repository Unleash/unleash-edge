#![cfg(feature = "s3-persistence")]
pub mod s3_persister {

    use ahash::HashMap;

    use async_trait::async_trait;
    use unleash_types::client_features::ClientFeatures;

    use crate::{EdgePersistence, EnterpriseEdgeLicenseState};
    use aws_sdk_s3::{
        self as s3,
        primitives::{ByteStream, SdkBody},
    };
    use unleash_edge_types::EdgeResult;
    use unleash_edge_types::errors::EdgeError;
    use unleash_edge_types::tokens::EdgeToken;

    pub const FEATURES_KEY: &str = "/unleash-features.json";
    pub const TOKENS_KEY: &str = "/unleash-tokens.json";
    pub const LICENSE_STATE_KEY: &str = "/unleash-license-state.json";

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
                .await
                .map_err(|_| EdgeError::PersistenceError("Failed to GET tokens".to_string()))?;
            let data = response.body.collect().await.expect("Failed data");
            serde_json::from_slice(&data.to_vec()).map_err(|e| {
                EdgeError::PersistenceError(format!("Failed to deserialize tokens: {}", e))
            })
        }

        async fn save_tokens(&self, tokens: Vec<EdgeToken>) -> EdgeResult<()> {
            let body_data = serde_json::to_vec(&tokens)
                .map_err(|e| {
                    EdgeError::PersistenceError(format!("Failed to serialize tokens: {}", e))
                })
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
                .map_err(|err| {
                    EdgeError::PersistenceError(format!(
                        "Failed to save tokens: {}",
                        err.into_service_error()
                    ))
                })
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
                Err(_e) => Ok(HashMap::default()),
            }
        }

        async fn save_features(&self, features: Vec<(String, ClientFeatures)>) -> EdgeResult<()> {
            let body_data = serde_json::to_vec(&features).map_err(|e| {
                EdgeError::PersistenceError(format!("Failed to serialize features: {}", e))
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
                Err(s3_err) => Err(EdgeError::PersistenceError(format!(
                    "Failed to save features: {}",
                    s3_err.into_service_error()
                ))),
            }
        }

        async fn load_license_state(&self) -> EnterpriseEdgeLicenseState {
            let Ok(response) = self
                .client
                .get_object()
                .bucket(self.bucket.clone())
                .key(LICENSE_STATE_KEY)
                .response_content_type("application/json")
                .send()
                .await else {
                    return EnterpriseEdgeLicenseState::Undetermined;
                };
              let Ok(data) = response.body.collect().await else {
                  return EnterpriseEdgeLicenseState::Undetermined;
              };
              serde_json::from_slice::<EnterpriseEdgeLicenseState>(&data.to_vec())
                  .unwrap_or(EnterpriseEdgeLicenseState::Undetermined)
            }

        async fn save_license_state(&self, license_state: &EnterpriseEdgeLicenseState) -> EdgeResult<()> {
            let body_data = serde_json::to_vec(&license_state).map_err(|e| {
                EdgeError::PersistenceError(format!("Failed to serialize license state: {}", e))
            })?;
            let byte_stream = ByteStream::new(SdkBody::from(body_data));
            match self
                .client
                .put_object()
                .bucket(self.bucket.clone())
                .key(LICENSE_STATE_KEY)
                .body(byte_stream)
                .send()
                .await
            {
                Ok(_) => Ok(()),
                Err(s3_err) => Err(EdgeError::PersistenceError(format!(
                    "Failed to save license state: {}",
                    s3_err.into_service_error()
                ))),
            }
        }
    }
}
