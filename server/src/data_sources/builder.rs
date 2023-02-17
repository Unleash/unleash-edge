use std::{sync::Arc, cell::RefCell};

use reqwest::Url;
use tokio::sync::{mpsc, RwLock};

use crate::{
    auth::token_validator::TokenValidator,
    cli::{CliArgs, EdgeArg, EdgeMode, OfflineArgs},
    http::unleash_client::UnleashClient,
    types::{EdgeResult, EdgeSink, EdgeSource, EdgeToken},
};

use super::{
    memory_provider::MemoryProvider, offline_provider::OfflineProvider,
    redis_provider::RedisProvider, repository::SinkFacade, repository::SourceFacade,
};

pub type DataProviderPair = (Arc<dyn EdgeSource>, Arc<dyn EdgeSink>);

pub struct RepositoryInfo {
    pub source: Arc<dyn EdgeSource>,
    pub sink_info: Option<SinkInfo>,
}

pub struct SinkInfo {
    pub sink: Arc<dyn EdgeSink>,
    pub validated_send: mpsc::Sender<EdgeToken>,
    pub validated_receive: mpsc::Receiver<EdgeToken>,
    pub unvalidated_receive: mpsc::Receiver<EdgeToken>,
    pub unleash_client: UnleashClient,
    pub token_validator: Arc<RwLock<TokenValidator>>,
    pub metrics_interval_seconds: u64,
}

fn build_offline(offline_args: OfflineArgs) -> EdgeResult<Arc<dyn EdgeSource>> {
    let provider =
        OfflineProvider::instantiate_provider(offline_args.bootstrap_file, offline_args.tokens)?;

    let token_source = Arc::new(RwLock::new(provider.clone()));
    let feature_source = Arc::new(RwLock::new(provider.clone()));

    let facade = Arc::new(SourceFacade {
        token_source,
        feature_source,
    });

    Ok(facade)
}

fn build_memory(features_refresh_interval_seconds: i64) -> EdgeResult<DataProviderPair> {
    let data_source = Arc::new(RwLock::new(MemoryProvider::new(
        features_refresh_interval_seconds,
    )));
    let source_facade = Arc::new(SourceFacade {
        token_source: data_source.clone(),
        feature_source: data_source.clone(),
    });
    let sink_facade = Arc::new(SinkFacade {
        token_sink: data_source.clone(),
        feature_sink: data_source.clone(),
    });
    Ok((source_facade, sink_facade))
}

fn build_redis(
    redis_url: String,
    features_refresh_interval_seconds: i64,
) -> EdgeResult<DataProviderPair> {
    let data_source = Arc::new(RwLock::new(RedisProvider::new(
        &redis_url,
        features_refresh_interval_seconds,
    )?));
    let source_facade = Arc::new(SourceFacade {
        token_source: data_source.clone(),
        feature_source: data_source.clone(),
    });
    let sink_facade = Arc::new(SinkFacade {
        token_sink: data_source.clone(),
        feature_sink: data_source.clone(),
    });
    Ok((source_facade, sink_facade))
}

pub async fn build_source_and_sink(args: CliArgs) -> EdgeResult<RepositoryInfo> {
    match args.mode {
        EdgeMode::Offline(offline_args) => {
            let source: Arc<dyn EdgeSource> = build_offline(offline_args)?;
            Ok(RepositoryInfo {
                source,
                sink_info: None,
            })
        }
        EdgeMode::Edge(edge_args) => {
            let arg: EdgeArg = edge_args.clone().into();
            let unleash_client = UnleashClient::from_url(
                Url::parse(edge_args.upstream_url.as_str()).expect("Cannot parse Upstream URL"),
            );
            let (unvalidated_sender, unvalidated_receiver) = mpsc::channel::<EdgeToken>(32);
            let (validated_sender, validated_receiver) = mpsc::channel::<EdgeToken>(32);
            let (source, sink) = match arg {
                EdgeArg::Redis(redis_url) => {
                    build_redis(redis_url, edge_args.features_refresh_interval_seconds)
                }
                EdgeArg::InMemory => build_memory(edge_args.features_refresh_interval_seconds),
            }?;

            let mut token_validator = TokenValidator {
                unleash_client: Arc::new(unleash_client.clone()),
                edge_source: source.clone(),
                edge_sink: sink.clone(),
            };
            if !edge_args.tokens.is_empty() {
                let _ = token_validator.register_tokens(edge_args.tokens).await;
            }
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
