use std::sync::Arc;

use reqwest::Url;
use tokio::sync::mpsc;

use tokio::sync::mpsc::Sender;
use tokio::sync::RwLock;

use crate::{
    auth::token_validator::TokenValidator,
    cli::{CliArgs, EdgeArg, EdgeMode, OfflineArgs},
    http::unleash_client::UnleashClient,
    types::{EdgeResult, EdgeSink, EdgeSource, EdgeToken},
};

use super::{
    memory_provider::MemoryProvider, offline_provider::OfflineProvider,
    redis_provider::RedisProvider,
};

pub type DataProviderPair = (Arc<RwLock<dyn EdgeSource>>, Arc<RwLock<dyn EdgeSink>>);

pub struct RepositoryInfo {
    pub source: Arc<RwLock<dyn EdgeSource>>,
    pub sink_info: Option<SinkInfo>,
}

pub struct SinkInfo {
    pub sink: Arc<RwLock<dyn EdgeSink>>,
    pub validated_send: mpsc::Sender<EdgeToken>,
    pub validated_receive: mpsc::Receiver<EdgeToken>,
    pub unvalidated_receive: mpsc::Receiver<EdgeToken>,
    pub unleash_client: UnleashClient,
    pub token_validator: Arc<RwLock<TokenValidator>>,
    pub metrics_interval_seconds: u64,
}

fn build_offline(offline_args: OfflineArgs) -> EdgeResult<Arc<RwLock<dyn EdgeSource>>> {
    let provider = OfflineProvider::instantiate_provider(
        offline_args.bootstrap_file,
        offline_args.client_keys,
    )?;
    let provider = Arc::new(RwLock::new(provider));
    Ok(provider)
}

fn build_memory(_sender: Sender<EdgeToken>) -> EdgeResult<DataProviderPair> {
    let data_source = Arc::new(RwLock::new(MemoryProvider::new()));
    Ok((data_source.clone(), data_source))
}

fn build_redis(redis_url: String, _sender: Sender<EdgeToken>) -> EdgeResult<DataProviderPair> {
    let data_source = Arc::new(RwLock::new(RedisProvider::new(&redis_url)?));
    Ok((data_source.clone(), data_source))
}

pub fn build_source_and_sink(args: CliArgs) -> EdgeResult<RepositoryInfo> {
    match args.mode {
        EdgeMode::Offline(offline_args) => {
            let source = build_offline(offline_args)?;
            Ok(RepositoryInfo {
                source,
                sink_info: None,
            })
        }
        EdgeMode::Edge(edge_args) => {
            let arg: EdgeArg = edge_args.clone().into();
            let unleash_client = UnleashClient::from_url(
                Url::parse(edge_args.unleash_url.as_str()).expect("Cannot parse Unleash URL"),
            );
            let (unvalidated_sender, unvalidated_receiver) = mpsc::channel::<EdgeToken>(32);
            let (validated_sender, validated_receiver) = mpsc::channel::<EdgeToken>(32);
            let (source, sink) = match arg {
                EdgeArg::Redis(redis_url) => build_redis(redis_url, unvalidated_sender),
                EdgeArg::InMemory => build_memory(unvalidated_sender),
            }?;
            let token_validator = TokenValidator {
                unleash_client: Arc::new(unleash_client.clone()),
                edge_source: source.clone(),
                edge_sink: sink.clone(),
            };

            Ok(RepositoryInfo {
                source,
                sink_info: Some(SinkInfo {
                    sink,
                    validated_send: validated_sender,
                    validated_receive: validated_receiver,
                    unvalidated_receive: unvalidated_receiver,
                    unleash_client,
                    token_validator: Arc::new(RwLock::new(token_validator)),
                    metrics_interval_seconds: edge_args.metrics_interval_seconds,
                }),
            })
        }
    }
}
