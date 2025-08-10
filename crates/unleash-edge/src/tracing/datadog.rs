use opentelemetry::global;
use opentelemetry_datadog::{new_pipeline, ApiVersion};
use opentelemetry_sdk::{trace::{self, RandomIdGenerator, Sampler}, Resource};
use tracing_subscriber::layer::SubscriberExt;
use unleash_edge_cli::CliArgs;
use unleash_edge_types::BuildInfo;
use crate::tracing::{formatting_layer, log_filter};

pub async fn configure_data_dog_tracer(cli_args: &CliArgs, build_info: BuildInfo) {
    let mut config = trace::Config::default();
    config.sampler = Box::new(Sampler::AlwaysOn);
    config.id_generator = Box::new(RandomIdGenerator::default());
    let provider = new_pipeline()
        .with_service_name(build_info.app_name)
        .with_api_version(ApiVersion::Version05)
        .with_agent_endpoint(cli_args.datadog_config.clone().datadog_url.unwrap_or("http://localhost:8126".to_string()))
        .with_trace_config(config)
        .install_batch().unwrap();
    global::set_tracer_provider(provider);
    tracing_subscriber::registry()
        .with(formatting_layer(cli_args))
        .with(log_filter());


}