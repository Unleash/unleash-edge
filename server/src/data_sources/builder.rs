use std::sync::{Arc, RwLock};

use crate::{
    cli::{CliArgs, EdgeArg, EdgeMode, OfflineArgs},
    types::{EdgeResult, EdgeSink, EdgeSource},
};

use super::{
    memory_provider::MemoryProvider, offline_provider::OfflineProvider,
    redis_provider::RedisProvider,
};

pub type DataProviderPair = (Arc<RwLock<dyn EdgeSource>>, Arc<RwLock<dyn EdgeSink>>);

fn build_offline(offline_args: OfflineArgs) -> EdgeResult<DataProviderPair> {
    let provider = OfflineProvider::instantiate_provider(
        offline_args.bootstrap_file,
        offline_args.client_keys,
    )?;
    let provider = Arc::new(RwLock::new(provider));
    Ok((provider.clone(), provider))
}

fn build_memory() -> EdgeResult<DataProviderPair> {
    let data_source = Arc::new(RwLock::new(MemoryProvider::default()));
    Ok((data_source.clone(), data_source))
}

fn build_redis(redis_url: String) -> EdgeResult<DataProviderPair> {
    let data_source = Arc::new(RwLock::new(RedisProvider::new(&redis_url)?));
    Ok((data_source.clone(), data_source))
}

pub fn build_source_and_sink(args: CliArgs) -> EdgeResult<DataProviderPair> {
    match args.mode {
        EdgeMode::Offline(offline_args) => build_offline(offline_args),
        EdgeMode::Edge(edge_args) => {
            let arg: EdgeArg = edge_args.into();
            match arg {
                EdgeArg::Redis(redis_url) => build_redis(redis_url),
                EdgeArg::InMemory => build_memory(),
            }
        }
    }
}
