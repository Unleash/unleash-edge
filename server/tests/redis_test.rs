use std::fs;

use redis::{Client, Commands};
use testcontainers::{clients::Cli, images::redis::Redis, Container};
use unleash_edge::{
    data_sources::redis_provider::{RedisProvider, FEATURE_KEY, TOKENS_KEY},
    types::{EdgeProvider, EdgeToken},
};

const TOKEN: &str = "*:development.03fa5f506428fe80ed5640c351c7232e38940814d2923b08f5c05fa7";

fn setup_redis(docker: &Cli) -> (Client, String, Container<Redis>) {
    let node: Container<Redis> = docker.run(Redis::default());
    let host_port = node.get_host_port_ipv4(6379);
    let url = format!("redis://127.0.0.1:{host_port}");

    (redis::Client::open(url.clone()).unwrap(), url, node)
}

#[tokio::test]
async fn redis_provider_returns_expected_data() {
    let docker = Cli::default();
    let (mut client, url, _node) = setup_redis(&docker);

    let content = fs::read_to_string("../examples/features.json").expect("Could not read file");

    //This wants a type hint but we don't care about the result so we immediately discard the data coming back
    let _: () = client.set(FEATURE_KEY, content).unwrap();

    let provider: Box<dyn EdgeProvider> = Box::new(RedisProvider::new(&url).unwrap());

    let features = provider
        .get_client_features(EdgeToken::try_from(TOKEN.to_string()).unwrap())
        .unwrap();

    assert!(!features.features.is_empty());
}

#[tokio::test]
async fn redis_provider_returns_token_info() {
    let docker = Cli::default();
    let (mut client, url, _node) = setup_redis(&docker);

    let _: () = client.set(TOKENS_KEY, format!("[\"{TOKEN}\"]")).unwrap();

    let provider: Box<dyn EdgeProvider> = Box::new(RedisProvider::new(&url).unwrap());

    let tokens = provider.get_known_tokens().unwrap();
    assert_eq!(
        *tokens[0].environment.as_ref().unwrap(),
        "development".to_string()
    );
}

#[tokio::test]
async fn redis_provider_correctly_determines_secret_to_be_valid() {
    let docker = Cli::default();
    let (mut client, url, _node) = setup_redis(&docker);

    let _: () = client.set(TOKENS_KEY, format!("[\"{TOKEN}\"]")).unwrap();

    let provider: Box<dyn EdgeProvider> = Box::new(RedisProvider::new(&url).unwrap());

    let is_valid_token = provider.secret_is_valid(TOKEN).unwrap();
    assert!(is_valid_token)
}
