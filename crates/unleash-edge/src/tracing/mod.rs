use tracing_subscriber::{EnvFilter, Layer, Registry};
use unleash_edge_cli::{CliArgs, LogFormat};

#[cfg(feature ="tracing-datadog")]
pub mod datadog;
#[cfg(feature = "tracing-sentry")]
pub mod sentry;
#[cfg(feature = "tracing-otlp")]
pub mod otlp;

pub fn log_filter() -> EnvFilter {
    EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("info"))
        .unwrap()
}
pub fn formatting_layer(cli_args: &CliArgs) -> Box<dyn Layer<Registry> + Send + Sync> {
    Box::new(match &cli_args.log_format {
        LogFormat::Plain => tracing_subscriber::fmt::layer().boxed(),
        LogFormat::Json => tracing_subscriber::fmt::layer().json().boxed(),
        LogFormat::Pretty => tracing_subscriber::fmt::layer().pretty().boxed(),
    })
}