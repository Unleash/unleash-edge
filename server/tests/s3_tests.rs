#[cfg(all(test, feature = "s3-persistence"))]
mod s3_tests {

    use std::collections::HashMap;
    use std::str::FromStr;

    use aws_config::Region;
    use aws_sdk_s3 as s3;
    use aws_sdk_s3::config::Credentials;
    use aws_sdk_s3::config::SharedCredentialsProvider;
    use testcontainers::{ImageExt, runners::AsyncRunner};
    use testcontainers_modules::localstack::LocalStack;
    use unleash_edge::persistence::EdgePersistence;
    use unleash_edge::persistence::s3::s3_persister::S3Persister;
    use unleash_edge::types::EdgeToken;
    use unleash_types::client_features::ClientFeature;
    use unleash_types::client_features::ClientFeatures;

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
            meta: None,
        };
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
        let persister = S3Persister::new_with_config(bucket_name, config);

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
}
