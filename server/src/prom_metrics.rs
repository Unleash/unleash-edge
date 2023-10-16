use opentelemetry_sdk::metrics::MeterProvider;
#[cfg(target_os = "linux")]
use prometheus::process_collector::ProcessCollector;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{EnvFilter, Registry};

use crate::http::background_send_metrics;
use crate::metrics::actix_web_metrics::{
    PrometheusMetricsHandler, RequestMetrics, RequestMetricsBuilder,
};

fn instantiate_tracing_and_logging() {
    let logger = tracing_subscriber::fmt::layer();
    let env_filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("info"))
        .unwrap();
    let collector = Registry::default().with(logger).with(env_filter);
    // Initialize tracing
    tracing::subscriber::set_global_default(collector).unwrap();
}

pub fn instantiate(
    registry: Option<prometheus::Registry>,
) -> (PrometheusMetricsHandler, RequestMetrics) {
    instantiate_tracing_and_logging();
    let registry = registry.unwrap_or_else(instantiate_registry);
    register_custom_metrics(&registry);
    instantiate_prometheus_metrics_handler(registry)
}

fn instantiate_prometheus_metrics_handler(
    registry: prometheus::Registry,
) -> (PrometheusMetricsHandler, RequestMetrics) {
    let resource = opentelemetry::sdk::Resource::new(vec![
        opentelemetry::KeyValue::new("service.name", "unleash-edge"),
        opentelemetry::KeyValue::new("edge.version", crate::types::build::PKG_VERSION),
        opentelemetry::KeyValue::new("edge.githash", crate::types::build::SHORT_COMMIT),
    ]);
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
