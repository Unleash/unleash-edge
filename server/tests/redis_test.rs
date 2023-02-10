use std::str::FromStr;

use redis::{Client, Commands};
use testcontainers::{clients::Cli, images::redis::Redis, Container};
use tokio::sync::mpsc;

use unleash_edge::{
    data_sources::redis_provider::{RedisProvider, FEATURE_PREFIX},
    types::{EdgeSink, EdgeSource, EdgeToken, TokenValidationStatus},
};
use unleash_types::client_features::{ClientFeature, ClientFeatures};

const TOKEN: &str = "*:development.03fa5f506428fe80ed5640c351c7232e38940814d2923b08f5c05fa7";

fn setup_redis(docker: &Cli) -> (Client, String, Container<Redis>) {
    let node: Container<Redis> = docker.run(Redis::default());
    let host_port = node.get_host_port_ipv4(6379);
    let url = format!("redis://127.0.0.1:{host_port}");

    (redis::Client::open(url.clone()).unwrap(), url, node)
}

fn build_features_key(token: &EdgeToken) -> String {
    token
        .environment
        .as_ref()
        .map(|environment| format!("{FEATURE_PREFIX}{environment}"))
        .expect("Tying to resolve features for a token that hasn't been validated")
}

#[tokio::test]
async fn redis_sink_returns_stores_data_correctly() {
    let docker = Cli::default();
    let (mut client, url, _node) = setup_redis(&docker);

    let (send, _) = mpsc::channel::<EdgeToken>(32);

    let mut sink: Box<dyn EdgeSink> = Box::new(RedisProvider::new(&url, send).unwrap());

    let token = EdgeToken {
        status: TokenValidationStatus::Validated,
        environment: Some("some-env-1".to_string()),
        projects: vec!["default".to_string()],
        ..EdgeToken::from_str(TOKEN).unwrap()
    };

    let features = ClientFeatures {
        features: vec![ClientFeature {
            name: "test".to_string(),
            ..ClientFeature::default()
        }],
        query: None,
        segments: None,
        version: 2,
    };

    let key = build_features_key(&token);

    sink.sink_features(&token, features.clone()).await.unwrap();
    let stored_features: String = client.get::<&str, String>(key.as_str()).unwrap();
    let stored_features: ClientFeatures = serde_json::from_str(&stored_features).unwrap();
    assert_eq!(stored_features, features.clone());
}

#[tokio::test]
async fn redis_sink_returns_merges_features_by_environment() {
    let docker = Cli::default();
    let (mut client, url, _node) = setup_redis(&docker);

    let (send, _) = mpsc::channel::<EdgeToken>(32);

    let mut sink: Box<dyn EdgeSink> = Box::new(RedisProvider::new(&url, send).unwrap());

    let token = EdgeToken {
        environment: Some("some-env-2".to_string()),
        status: TokenValidationStatus::Validated,
        projects: vec!["default".to_string()],
        ..EdgeToken::from_str(TOKEN).unwrap()
    };

    let key = build_features_key(&token);

    let features1 = ClientFeatures {
        features: vec![ClientFeature {
            name: "test".to_string(),
            ..ClientFeature::default()
        }],
        query: None,
        segments: None,
        version: 2,
    };

    sink.sink_features(&token, features1.clone()).await.unwrap();

    let features2 = ClientFeatures {
        features: vec![ClientFeature {
            name: "some-other-test".to_string(),
            ..ClientFeature::default()
        }],
        query: None,
        segments: None,
        version: 2,
    };

    sink.sink_features(&token, features2.clone()).await.unwrap();

    let first_expected_toggle = ClientFeature {
        name: "some-other-test".to_string(),
        ..ClientFeature::default()
    };

    let second_expected_toggle = ClientFeature {
        name: "test".to_string(),
        ..ClientFeature::default()
    };

    let stored_features: String = client.get::<&str, String>(key.as_str()).unwrap();
    let stored_features: ClientFeatures = serde_json::from_str(&stored_features).unwrap();
    assert!(stored_features.features.contains(&first_expected_toggle));
    assert!(stored_features.features.contains(&second_expected_toggle));
}

#[tokio::test]
async fn redis_sink_returns_splits_out_data_with_different_environments() {
    let docker = Cli::default();
    let (mut client, url, _node) = setup_redis(&docker);

    let (send, _) = mpsc::channel::<EdgeToken>(32);

    let mut sink: Box<dyn EdgeSink> = Box::new(RedisProvider::new(&url, send).unwrap());

    let dev_token = EdgeToken {
        status: TokenValidationStatus::Validated,
        environment: Some("some-env-3".to_string()),
        projects: vec!["default".to_string()],
        ..EdgeToken::from_str(TOKEN).unwrap()
    };

    let prod_token = EdgeToken {
        status: TokenValidationStatus::Validated,
        environment: Some("some-env-4".to_string()),
        projects: vec!["default".to_string()],
        ..EdgeToken::from_str(TOKEN).unwrap()
    };

    let dev_key = build_features_key(&dev_token);

    let features1 = ClientFeatures {
        features: vec![ClientFeature {
            name: "test".to_string(),
            ..ClientFeature::default()
        }],
        query: None,
        segments: None,
        version: 2,
    };

    sink.sink_features(&dev_token, features1.clone())
        .await
        .unwrap();

    let features2 = ClientFeatures {
        features: vec![ClientFeature {
            name: "some-other-test".to_string(),
            ..ClientFeature::default()
        }],
        query: None,
        segments: None,
        version: 2,
    };

    sink.sink_features(&prod_token, features2.clone())
        .await
        .unwrap();

    let expected = ClientFeatures {
        features: vec![ClientFeature {
            name: "test".to_string(),
            ..ClientFeature::default()
        }],
        query: None,
        segments: None,
        version: 2,
    };

    let stored_features: String = client.get::<&str, String>(dev_key.as_str()).unwrap();
    let stored_features: ClientFeatures = serde_json::from_str(&stored_features).unwrap();
    assert_eq!(stored_features, expected);
}

#[tokio::test]
async fn redis_source_filters_by_projects() {
    let docker = Cli::default();
    let (_client, url, _node) = setup_redis(&docker);

    let (send, _) = mpsc::channel::<EdgeToken>(32);
    let (other_send, _) = mpsc::channel::<EdgeToken>(32);

    let source: Box<dyn EdgeSource> = Box::new(RedisProvider::new(&url, send).unwrap());
    let mut sink: Box<dyn EdgeSink> = Box::new(RedisProvider::new(&url, other_send).unwrap());

    let features = ClientFeatures {
        features: vec![
            ClientFeature {
                name: "some-other-test".to_string(),
                project: Some("some-project".to_string()),
                ..ClientFeature::default()
            },
            ClientFeature {
                name: "some-other-test".to_string(),
                project: Some("some-other-project".to_string()),
                ..ClientFeature::default()
            },
        ],
        query: None,
        segments: None,
        version: 2,
    };

    let token = EdgeToken {
        status: TokenValidationStatus::Validated,
        environment: Some("some-env-5".to_string()),
        projects: vec!["some-project".to_string()],
        ..EdgeToken::from_str(TOKEN).unwrap()
    };

    let expected = ClientFeatures {
        features: vec![ClientFeature {
            name: "some-other-test".to_string(),
            project: Some("some-project".to_string()),
            ..ClientFeature::default()
        }],
        query: None,
        segments: None,
        version: 2,
    };

    sink.sink_features(&token, features.clone()).await.unwrap();

    let stored_features = source.get_client_features(&token).await.unwrap();
    assert_eq!(stored_features, expected);
}
