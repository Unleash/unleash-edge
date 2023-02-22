use std::str::FromStr;

use actix_web::http::header::EntityTag;
use redis::Client;
use testcontainers::{clients::Cli, images::redis::Redis, Container};

use unleash_edge::{
    data_sources::{
        redis_provider::RedisProvider,
        repository::{DataSink, DataSource},
    },
    types::{EdgeToken, TokenRefresh, TokenValidationStatus},
};
use unleash_types::client_features::{ClientFeature, ClientFeatures};

const TOKEN: &str = "*:development.03fa5f506428fe80ed5640c351c7232e38940814d2923b08f5c05fa7";

fn setup_redis(docker: &Cli) -> (Client, String, Container<Redis>) {
    let node: Container<Redis> = docker.run(Redis::default());
    let host_port = node.get_host_port_ipv4(6379);
    let url = format!("redis://127.0.0.1:{host_port}");

    (redis::Client::open(url.clone()).unwrap(), url, node)
}

#[tokio::test]
async fn redis_stores_and_returns_data_correctly() {
    let docker = Cli::default();
    let (_client, url, _node) = setup_redis(&docker);

    let mut redis: RedisProvider = RedisProvider::new(&url).unwrap();

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

    redis.sink_features(&token, features.clone()).await.unwrap();

    let expected_features = redis.get_client_features(&token).await.unwrap().unwrap();

    assert_eq!(expected_features, features.clone());
}

#[tokio::test]
async fn redis_stores_and_returns_tokens_correctly() {
    let docker = Cli::default();
    let (_client, url, _node) = setup_redis(&docker);

    let mut redis: RedisProvider = RedisProvider::new(&url).unwrap();

    let token = EdgeToken {
        status: TokenValidationStatus::Validated,
        environment: Some("some-env-1".to_string()),
        projects: vec!["default".to_string()],
        ..EdgeToken::from_str(TOKEN).unwrap()
    };

    let tokens = vec![token];

    redis.sink_tokens(tokens.clone()).await.unwrap();
    let returned_token = redis.get_token(TOKEN).await.unwrap().unwrap();
    assert_eq!(returned_token, tokens[0]);
}

#[tokio::test]
async fn redis_stores_and_returns_refresh_tokens_correctly() {
    let docker = Cli::default();
    let (_client, url, _node) = setup_redis(&docker);

    let mut redis: RedisProvider = RedisProvider::new(&url).unwrap();

    let tokens = vec![TokenRefresh {
        etag: None,
        last_refreshed: None,
        last_check: None,
        token: EdgeToken {
            status: TokenValidationStatus::Validated,
            environment: Some("some-env-1".to_string()),
            projects: vec!["default".to_string()],
            ..EdgeToken::from_str(TOKEN).unwrap()
        },
    }];

    redis
        .set_refresh_tokens(tokens.iter().collect::<Vec<&TokenRefresh>>())
        .await
        .unwrap();
    let returned_tokens = redis.get_refresh_tokens().await.unwrap();
    assert_eq!(returned_tokens[0].token, tokens[0].token);
}

#[tokio::test]
async fn redis_store_marks_update_correctly() {
    let docker = Cli::default();
    let (_client, url, _node) = setup_redis(&docker);

    let mut redis: RedisProvider = RedisProvider::new(&url).unwrap();

    let token = EdgeToken {
        status: TokenValidationStatus::Validated,
        environment: Some("some-env-1".to_string()),
        projects: vec!["default".to_string()],
        ..EdgeToken::from_str(TOKEN).unwrap()
    };

    let entity_tag = EntityTag::new_weak("some-etag".to_string());
    let token_refresh = TokenRefresh {
        etag: None,
        last_refreshed: None,
        last_check: None,
        token: token.clone(),
    };

    let tokens = vec![token_refresh.clone()];

    redis
        .set_refresh_tokens(tokens.iter().collect::<Vec<&TokenRefresh>>())
        .await
        .unwrap();

    redis
        .update_last_refresh(&token, Some(entity_tag.clone()))
        .await
        .unwrap();

    let found_token = redis
        .get_refresh_tokens()
        .await
        .unwrap()
        .get(0)
        .unwrap()
        .clone();

    assert_eq!(found_token.etag, Some(entity_tag));
    assert!(found_token.last_check.is_some());
    assert!(found_token.last_refreshed.is_some());
}
