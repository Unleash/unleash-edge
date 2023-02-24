use actix_web_opentelemetry::{PrometheusMetricsHandler, RequestMetrics, RequestMetricsBuilder};
use opentelemetry::{
    global,
    sdk::{
        export::metrics::aggregation,
        metrics::{controllers, processors, selectors},
    },
    trace::TraceId,
};
#[cfg(target_os = "linux")]
use prometheus::process_collector::ProcessCollector;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{EnvFilter, Registry};

#[cfg(feature = "telemetry")]
use tracing_opentelemetry;

async fn instantiate_tracing_and_logging() {
    #[cfg(feature = "telemetry")]
    let telemetry = tracing_opentelemetry::layer().with_tracer(init_tracer().await);

    let logger = tracing_subscriber::fmt::layer();
    let env_filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("info"))
        .unwrap();

    // Decide on layers
    #[cfg(feature = "telemetry")]
    let collector = Registry::default()
        .with(telemetry)
        .with(logger)
        .with(env_filter);
    #[cfg(not(feature = "telemetry"))]
    let collector = Registry::default().with(logger).with(env_filter);
    // Initialize tracing
    tracing::subscriber::set_global_default(collector).unwrap();
}

pub async fn instantiate(
    registry: Option<prometheus::Registry>,
) -> (PrometheusMetricsHandler, RequestMetrics) {
    instantiate_tracing_and_logging().await;
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
    .with_resource(opentelemetry::sdk::Resource::new(vec![
        opentelemetry::KeyValue::new("service.name", "unleash-edge"),
        opentelemetry::KeyValue::new("edge.version", crate::types::build::PKG_VERSION),
        opentelemetry::KeyValue::new("edge.githash", crate::types::build::SHORT_COMMIT),
    ]))
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

pub fn get_trace_id() -> TraceId {
    use opentelemetry::trace::TraceContextExt as _; // opentelemetry::Context -> opentelemetry::trace::Span
    use tracing_opentelemetry::OpenTelemetrySpanExt as _; // tracing::Span to opentelemetry::Context

    tracing::Span::current()
        .context()
        .span()
        .span_context()
        .trace_id()
}

#[cfg(feature = "telemetry")]
pub async fn init_tracer() -> opentelemetry::sdk::trace::Tracer {
    use opentelemetry::sdk::trace::RandomIdGenerator;
    let otlp_endpoint = std::env::var("OPENTELEMETRY_ENDPOINT_URL")
        .expect("Need a otel tracing collector configured");

    let channel = tonic::transport::Channel::from_shared(otlp_endpoint)
        .unwrap()
        .connect()
        .await
        .unwrap();

    opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(
            opentelemetry_otlp::new_exporter()
                .tonic()
                .with_channel(channel),
        )
        .with_trace_config(
            opentelemetry::sdk::trace::config()
                .with_resource(opentelemetry::sdk::Resource::new(vec![
                    opentelemetry::KeyValue::new("service.name", "unleash-edge"),
                ]))
                .with_id_generator(RandomIdGenerator::default()),
        )
        .install_batch(opentelemetry::runtime::Tokio)
        .unwrap()
}
