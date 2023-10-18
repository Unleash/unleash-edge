use opentelemetry_sdk::metrics::MeterProvider;

#[cfg(target_os = "linux")]
use prometheus::process_collector::ProcessCollector;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{EnvFilter, Registry};

use crate::http::background_send_metrics;
use crate::metrics::actix_web_metrics::{
    PrometheusMetricsHandler, RequestMetrics, RequestMetricsBuilder,
};

#[cfg(feature = "opentelemetry")]
use opentelemetry_otlp::WithExportConfig;

#[cfg(feature = "opentelemetry")]
use opentelemetry::{global, sdk::trace as sdktrace, sdk::Resource};

fn instantiate_tracing_and_logging() {
    #[cfg(feature = "newrelic")]
    {
        std::env::var("NEWRELIC_API_KEY")
            .map(|api_key| {
                let new_relic = tracing_newrelic::layer(api_key);
                let logger = tracing_subscriber::fmt::layer();
                let env_filter = EnvFilter::try_from_default_env()
                    .or_else(|_| EnvFilter::try_new("info"))
                    .unwrap();
                let collector = Registry::default()
                    .with(new_relic)
                    .with(logger)
                    .with(env_filter);
                // Initialize tracing
                tracing::subscriber::set_global_default(collector).unwrap();
                tracing::info!("Done setting up tracing with NewRelic layer");
            })
            .unwrap_or_else(|_| {
                let logger = tracing_subscriber::fmt::layer();
                let env_filter = EnvFilter::try_from_default_env()
                    .or_else(|_| EnvFilter::try_new("info"))
                    .unwrap();
                let collector = Registry::default().with(logger).with(env_filter);
                // Initialize tracing
                tracing::subscriber::set_global_default(collector).unwrap();
                tracing::warn!("NewRelic API key not set, not enabling NewRelic tracing");
                tracing::info!("Done setting up tracing with just a logger layer");
            })
    }
    #[cfg(feature = "opentelemetry")]
    {
        std::env::var("OTEL_COLLECTION_URL")
            .map(|url| {
                let tracer = opentelemetry_otlp::new_pipeline()
                    .tracing()
                    .with_exporter(
                        opentelemetry_otlp::new_exporter()
                            .tonic()
                            .with_endpoint(url),
                    )
                    .with_trace_config(sdktrace::config().with_resource(resource()))
                    .install_batch(opentelemetry::runtime::Tokio)
                    .expect("Failed to install tracing collector");
                let telemetry = tracing_opentelemetry::layer().with_tracer(tracer);
                let logger = tracing_subscriber::fmt::layer();
                let env_filter = EnvFilter::try_from_default_env()
                    .or_else(|_| EnvFilter::try_new("info"))
                    .unwrap();
                let collector = Registry::default()
                    .with(telemetry)
                    .with(logger)
                    .with(env_filter);
                // Initialize tracing
                tracing::subscriber::set_global_default(collector).unwrap();
                tracing::warn!("Opentelemetry tracing setup done");
            })
            .unwrap_or_else(|_| {})
    }
    #[cfg(not(any(feature = "newrelic", feature = "opentelemetry")))]
    {
        let logger = tracing_subscriber::fmt::layer();
        let env_filter = EnvFilter::try_from_default_env()
            .or_else(|_| EnvFilter::try_new("info"))
            .unwrap();
        let collector = Registry::default().with(logger).with(env_filter);
        // Initialize tracing
        tracing::subscriber::set_global_default(collector).unwrap();
        tracing::info!("Done setting up tracing with just a logger layer");
    }
}

pub fn instantiate(
    registry: Option<prometheus::Registry>,
) -> (PrometheusMetricsHandler, RequestMetrics) {
    instantiate_tracing_and_logging();
    let registry = registry.unwrap_or_else(instantiate_registry);
    register_custom_metrics(&registry);
    instantiate_prometheus_metrics_handler(registry)
}

fn resource() -> opentelemetry::sdk::Resource {
    opentelemetry::sdk::Resource::new(vec![
        opentelemetry::KeyValue::new("otel.name", "unleash-edge"),
        opentelemetry::KeyValue::new("service.name", "unleash-edge"),
        opentelemetry::KeyValue::new("edge.version", crate::types::build::PKG_VERSION),
        opentelemetry::KeyValue::new("edge.githash", crate::types::build::SHORT_COMMIT),
    ])
}

fn instantiate_prometheus_metrics_handler(
    registry: prometheus::Registry,
) -> (PrometheusMetricsHandler, RequestMetrics) {
    let resource = resource();
    let provider = MeterProvider::builder().with_resource(resource).build();
    (
        PrometheusMetricsHandler::new(registry),
        RequestMetricsBuilder::new()
            .with_meter_provider(provider)
            .build(),
    )
}

fn instantiate_registry() -> prometheus::Registry {
    #[cfg(target_os = "linux")]
    {
        let registry = prometheus::Registry::new();
        let process_collector = ProcessCollector::for_self();
        let _register_result = registry.register(Box::new(process_collector));
        registry
    }
    #[cfg(not(target_os = "linux"))]
    prometheus::Registry::new()
}

fn register_custom_metrics(registry: &prometheus::Registry) {
    registry
        .register(Box::new(
            background_send_metrics::METRICS_UNEXPECTED_ERRORS.clone(),
        ))
        .unwrap();
    registry
        .register(Box::new(
            background_send_metrics::METRICS_UPSTREAM_HTTP_ERRORS.clone(),
        ))
        .unwrap();
    registry
        .register(Box::new(
            crate::metrics::client_metrics::METRICS_SIZE_HISTOGRAM.clone(),
        ))
        .unwrap();
    registry
        .register(Box::new(
            crate::http::unleash_client::CLIENT_FEATURE_FETCH_FAILURES.clone(),
        ))
        .unwrap();
    registry
        .register(Box::new(
            crate::http::unleash_client::CLIENT_REGISTER_FAILURES.clone(),
        ))
        .unwrap();
    registry
        .register(Box::new(
            crate::http::unleash_client::TOKEN_VALIDATION_FAILURES.clone(),
        ))
        .unwrap();
    registry
        .register(Box::new(
            crate::http::unleash_client::CLIENT_FEATURE_FETCH.clone(),
        ))
        .unwrap();
}
