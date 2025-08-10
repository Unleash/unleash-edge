use crate::tracing::{formatting_layer, log_filter};
use opentelemetry::global;
use opentelemetry_sdk::trace::SdkTracerProvider;
use sentry::integrations::opentelemetry as sentry_opentelemetry;
use sentry::ClientInitGuard;
use sentry_tracing::EventFilter;
use tracing_subscriber::prelude::__tracing_subscriber_SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use unleash_edge_cli::CliArgs;

pub fn configure_sentry(cli_args: &CliArgs) -> ClientInitGuard {
    let guard = sentry::init((
        cli_args.sentry_config.sentry_dsn.clone().unwrap(),
        sentry::ClientOptions {
            traces_sample_rate: cli_args.sentry_config.sentry_tracing_rate,
            debug: cli_args.sentry_config.sentry_debug,
            enable_logs: cli_args.sentry_config.sentry_enable_logs,
            ..sentry::ClientOptions::default()
        }
        ));
    let sentry_layer = sentry::integrations::tracing::layer()
        .event_filter(|md| match *md.level() {
            tracing::Level::ERROR => EventFilter::Event,
            tracing::Level::WARN => EventFilter::Event,
            _ => EventFilter::Ignore,
        });

    global::set_text_map_propagator(sentry_opentelemetry::SentryPropagator::new());
    let tracer_provider = SdkTracerProvider::builder()
        .with_span_processor(sentry_opentelemetry::SentrySpanProcessor::new())
        .build();
    global::set_tracer_provider(tracer_provider);

    tracing_subscriber::registry()
        .with(formatting_layer(&cli_args))
        .with(sentry_layer)
        .with(log_filter())
        .init();

    guard
}