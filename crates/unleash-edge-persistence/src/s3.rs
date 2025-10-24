#![cfg(feature = "s3-persistence")]
pub mod s3_persister {

    use ahash::HashMap;

    use async_trait::async_trait;
    use unleash_types::client_features::ClientFeatures;

    use crate::EdgePersistence;
    use aws_sdk_s3::{
        self as s3,
        primitives::{ByteStream, SdkBody},
    };
    use unleash_edge_types::errors::EdgeError;
    use unleash_edge_types::tokens::EdgeToken;
    use unleash_edge_types::{EdgeResult, enterprise::LicenseState};

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

        async fn load_license_state(&self) -> LicenseState {
            let Ok(response) = self
                .client
                .get_object()
                .bucket(self.bucket.clone())
                .key(LICENSE_STATE_KEY)
                .response_content_type("application/json")
                .send()
                .await
            else {
                return LicenseState::Undetermined;
            };
            let Ok(data) = response.body.collect().await else {
                return LicenseState::Undetermined;
            };
            serde_json::from_slice::<LicenseState>(&data.to_vec())
                .unwrap_or(LicenseState::Undetermined)
        }

        async fn save_license_state(
            &self,
            license_state: &LicenseState,
        ) -> EdgeResult<()> {
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

#[cfg(test)]
mod tests {
    #[cfg(all(test, feature = "s3-persistence"))]
    mod s3_tests {

        use crate::EdgePersistence;
        use crate::s3::s3_persister::S3Persister;

        use ahash::HashMap;
        use aws_config::Region;
        use aws_sdk_s3 as s3;
        use aws_sdk_s3::config::Credentials;
        use aws_sdk_s3::config::SharedCredentialsProvider;
        use std::str::FromStr;
        use testcontainers::ContainerAsync;
        use testcontainers::{ImageExt, runners::AsyncRunner};
        use testcontainers_modules::localstack::LocalStack;
        use unleash_edge_types::tokens::EdgeToken;
        use unleash_types::client_features::ClientFeature;
        use unleash_types::client_features::ClientFeatures;

        async fn setup_s3_persister() -> (ContainerAsync<LocalStack>, S3Persister) {
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

            let config = s3::config::Config::builder()
                .region(Region::new("us-east-1"))
                .endpoint_url(format!("http://{}:{}", local_stack_ip, local_stack_port))
                .credentials_provider(SharedCredentialsProvider::new(Credentials::for_tests()))
                .force_path_style(true)
                .build();

            let client = s3::Client::from_conf(config.clone());
            client
                .create_bucket()
                .bucket(bucket_name)
                .send()
                .await
                .expect("Failed to setup S3 bucket pre test run");

            //hopefully we don't care, this should just work with localstack
            (
                localstack,
                S3Persister::new_with_config(bucket_name, config),
            )
        }

        #[tokio::test]
        async fn test_s3_persister() {
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
                meta: None,
            };
            let (_localstack, persister) = setup_s3_persister().await;

            let tokens = vec![EdgeToken::from_str("eg:development.secret321").unwrap()];
            persister.save_tokens(tokens.clone()).await.unwrap();

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

        #[tokio::test]
        async fn test_s3_persister_loads_license_state() {
            let (_localstack, persister) = setup_s3_persister().await;

            let loaded_license_state = persister.load_license_state().await;
            assert_eq!(
                loaded_license_state,
                crate::LicenseState::Undetermined
            );

            let license_state = crate::LicenseState::Valid;
            persister
                .save_license_state(&license_state)
                .await
                .expect("Failed to save license state");

            let loaded_license_state = persister.load_license_state().await;
            assert_eq!(loaded_license_state, license_state);
        }

        #[tokio::test]
        async fn test_s3_persister_returns_undetermined_when_no_data_present() {
            let (_localstack, persister) = setup_s3_persister().await;

            let loaded_license_state = persister.load_license_state().await;
            assert_eq!(
                loaded_license_state,
                crate::LicenseState::Undetermined
            );
        }
    }
}
