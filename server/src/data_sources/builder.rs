use std::sync::Arc;

use crate::{
    cli::{CliArgs, EdgeArg, EdgeMode, OfflineArgs},
    types::{EdgeResult, EdgeSink, EdgeSource},
};

use super::{
    memory_provider::MemoryProvider, offline_provider::OfflineProvider,
    redis_provider::RedisProvider,
};

fn build_offline(
    offline_args: OfflineArgs,
) -> EdgeResult<(Arc<dyn EdgeSource>, Arc<dyn EdgeSink>)> {
    let provider = OfflineProvider::instantiate_provider(
        offline_args.bootstrap_file,
        offline_args.client_keys,
    )
    .map(Arc::new)?;
    Ok((provider.clone(), provider.clone()))
}

fn build_memory() -> EdgeResult<(Arc<dyn EdgeSource>, Arc<dyn EdgeSink>)> {
    let data_source = MemoryProvider::default();
    Ok((Arc::new(data_source.clone()), Arc::new(data_source.clone())))
}

fn build_redis(redis_url: String) -> EdgeResult<(Arc<dyn EdgeSource>, Arc<dyn EdgeSink>)> {
    let data_source = RedisProvider::new(&redis_url).map(Arc::new)?;
    Ok((data_source.clone(), data_source.clone()))
}

pub fn build_source_and_sink(
    args: CliArgs,
) -> EdgeResult<(Arc<dyn EdgeSource>, Arc<dyn EdgeSink>)> {
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
