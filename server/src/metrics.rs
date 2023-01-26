use actix_web_opentelemetry::{PrometheusMetricsHandler, RequestMetrics, RequestMetricsBuilder};
use opentelemetry::{
    global,
    sdk::{
        export::metrics::aggregation,
        metrics::{controllers, processors, selectors},
    },
};
#[cfg(target_os = "linux")]
use prometheus::process_collector::ProcessCollector;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{EnvFilter, Registry};

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
    instantiate_prometheus_metrics_handler(registry)
}

fn instantiate_prometheus_metrics_handler(
    registry: prometheus::Registry,
) -> (PrometheusMetricsHandler, RequestMetrics) {
    let controller = controllers::basic(
        processors::factory(
            selectors::simple::histogram([0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 50.0]), // Will give histogram for with resolution in n ms
            aggregation::cumulative_temporality_selector(),
        )
        .with_memory(true),
    )
    .build();

    let exporter = opentelemetry_prometheus::exporter(controller)
        .with_registry(registry)
        .init();
    let meter = global::meter("edge_web");

    (
        PrometheusMetricsHandler::new(exporter),
        RequestMetricsBuilder::new().build(meter),
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
