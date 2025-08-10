use opentelemetry::{global, KeyValue};
use opentelemetry::trace::TracerProvider;
use opentelemetry_otlp::{SpanExporter, WithExportConfig};
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::trace::SdkTracerProvider;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use unleash_edge_cli::CliArgs;
use unleash_edge_types::BuildInfo;
use crate::tracing::{formatting_layer, log_filter};

pub fn configure_otlp(cli_args: &CliArgs) {
    let build_info: BuildInfo = BuildInfo::default();
    let exporter = SpanExporter::builder()
        .with_http()
        .build()
        .expect("Failed to create span exporter");
    let provider = SdkTracerProvider::builder()
        .with_resource(Resource::builder()
            .with_service_name(build_info.app_name)
            .with_attribute(KeyValue::new("version", build_info.package_version))
            .with_attribute(KeyValue::new("tag", build_info.tag))
            .build())
        .with_batch_exporter(exporter)
        .build();
    let tracer = provider.tracer("unleash_edge");
    let telemetry_layer = tracing_opentelemetry::layer().with_tracer(tracer);
    tracing_subscriber::registry()
        .with(log_filter())
        .with(tracing_subscriber::fmt::layer().pretty())
        .with(telemetry_layer)
        .init();
    global::set_text_map_propagator(TraceContextPropagator::new());
    global::set_tracer_provider(provider);

}