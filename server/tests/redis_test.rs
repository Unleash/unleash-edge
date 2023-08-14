use std::str::FromStr;

use actix_web::http::header::EntityTag;
use chrono::Utc;
use redis::Client;
use testcontainers::{clients::Cli, images::redis::Redis, Container};

use unleash_edge::{
    persistence::{redis::RedisPersister, EdgePersistence},
    types::{EdgeToken, TokenRefresh, TokenType},
};
use unleash_types::client_features::{ClientFeature, ClientFeatures};

fn setup_redis(docker: &Cli) -> (Client, String, Container<Redis>) {
    let node: Container<Redis> = docker.run(Redis);
    let host_port = node.get_host_port_ipv4(6379);
    let url = format!("redis://127.0.0.1:{host_port}");

    (redis::Client::open(url.clone()).unwrap(), url, node)
}

#[tokio::test]
async fn redis_saves_and_restores_features_correctly() {
    let docker = Cli::default();
    let (_client, url, _node) = setup_redis(&docker);
    let redis_persister = RedisPersister::new(&url).unwrap();

    let features = ClientFeatures {
        features: vec![ClientFeature {
            name: "test".to_string(),
            ..ClientFeature::default()
        }],
        query: None,
        segments: None,
        version: 2,
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
    let docker = Cli::default();
    let (_client, url, _node) = setup_redis(&docker);
    let redis_persister = RedisPersister::new(&url).unwrap();
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

#[tokio::test]
async fn redis_saves_and_restores_token_refreshes_correctly() {
    let docker = Cli::default();
    let (_client, url, _node) = setup_redis(&docker);
    let redis_persister = RedisPersister::new(&url).unwrap();
    let edge_token = EdgeToken::from_str("someproject:development.abcdefghijklmnopqr").unwrap();

    let mut token_refresh = TokenRefresh::new(edge_token.clone(), None);
    let now = Utc::now();
    token_refresh.last_check = Some(now);
    token_refresh.last_refreshed = Some(now);
    token_refresh.etag = Some(EntityTag::new_weak("abcdefghijl".into()));
    redis_persister
        .save_refresh_targets(vec![token_refresh])
        .await
        .unwrap();
    let saved_refreshes = redis_persister.load_refresh_targets().await.unwrap();
    assert_eq!(saved_refreshes.len(), 1);
    assert_eq!(saved_refreshes.get(0).unwrap().token, edge_token);
}
