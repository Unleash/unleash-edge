use std::{str::FromStr, time::Duration};

use redis::Client;
use testcontainers_modules::redis::RedisStack;
use unleash_types::client_features::{ClientFeature, ClientFeatures};

use testcontainers::{ContainerAsync, runners::AsyncRunner};
use unleash_edge::{
    persistence::{EdgePersistence, redis::RedisPersister},
    types::{EdgeToken, TokenType},
};

const TEST_TIMEOUT: Duration = std::time::Duration::from_millis(1000);

async fn setup_redis() -> (Client, String, ContainerAsync<RedisStack>) {
    let node = RedisStack::default()
        .start()
        .await
        .expect("Failed to start redis");
    let host_port = node
        .get_host_port_ipv4(6379)
        .await
        .expect("Could not get port");
    let url = format!("redis://127.0.0.1:{host_port}");

    (Client::open(url.clone()).unwrap(), url, node)
}

#[tokio::test]
async fn redis_saves_and_restores_features_correctly() {
    let (_client, url, _node) = setup_redis().await;
    let redis_persister = RedisPersister::new(&url, TEST_TIMEOUT, TEST_TIMEOUT).unwrap();

    let features = ClientFeatures {
        features: vec![ClientFeature {
            name: "test".to_string(),
            ..ClientFeature::default()
        }],
        query: None,
        segments: None,
        version: 2,
        meta: None,
    };
    let environment = "development";
    redis_persister
        .save_features(vec![(environment.into(), features.clone())])
        .await
        .unwrap();
    let results = redis_persister.load_features().await.unwrap();
    assert_eq!(results.get(environment).unwrap(), &features);
}

#[tokio::test]
async fn redis_saves_and_restores_edge_tokens_correctly() {
    let (_client, url, _node) = setup_redis().await;
    let redis_persister = RedisPersister::new(&url, TEST_TIMEOUT, TEST_TIMEOUT).unwrap();
    let mut project_specific_token =
        EdgeToken::from_str("someproject:development.abcdefghijklmnopqr").unwrap();
    project_specific_token.token_type = Some(TokenType::Client);
    let mut wildcard_token = EdgeToken::from_str("*:development.mysecretispersonal").unwrap();
    wildcard_token.token_type = Some(TokenType::Client);
    redis_persister
        .save_tokens(vec![project_specific_token, wildcard_token])
        .await
        .unwrap();
    let saved_tokens = redis_persister.load_tokens().await.unwrap();
    assert_eq!(saved_tokens.len(), 2);
}
