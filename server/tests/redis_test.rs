use std::fs;

use redis::Commands;
use testcontainers::{clients, images};
use unleash_edge::{
    data_sources::redis_provider::{RedisProvider, FEATURE_KEY},
    types::{EdgeProvider, EdgeToken},
};

#[tokio::test]
async fn redis_provider_returns_expected_data() {
    let docker = clients::Cli::default();
    let node = docker.run(images::redis::Redis::default());
    let host_port = node.get_host_port_ipv4(6379);
    let url = format!("redis://127.0.0.1:{}", host_port);

    let mut client = redis::Client::open(url.clone()).unwrap();

    let content =
        fs::read_to_string(format!("../examples/features.json")).expect("Could not read file");

    //Wants a type annotation but we don't care about the result so we immediately discard the data coming back
    let _: () = client.set(FEATURE_KEY, content).unwrap();

    let provider: Box<dyn EdgeProvider> = Box::new(RedisProvider::new(&url).unwrap());

    let features = provider
        .get_client_features(EdgeToken::try_from("secret-123".to_string()).unwrap())
        .unwrap();

    assert!(!features.features.is_empty());
}
