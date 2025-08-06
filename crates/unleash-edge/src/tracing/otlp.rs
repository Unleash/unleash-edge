use opentelemetry::{global, KeyValue};
use opentelemetry_otlp::SpanExporter;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::trace::SdkTracerProvider;
use unleash_edge_types::BuildInfo;

fn init_tracer_provider(build_info: BuildInfo) {
    let exporter = SpanExporter::builder()
        .with_tonic()
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
    global::set_text_map_propagator(TraceContextPropagator::new());
    global::set_tracer_provider(provider);
}