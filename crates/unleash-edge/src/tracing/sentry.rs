use opentelemetry::global;
use opentelemetry_sdk::trace::SdkTracerProvider;
use unleash_edge_types::BuildInfo;

pub struct SentryConfig {
    pub url: String,
    pub sample_rate: f32,
    pub debug: bool,
}

pub async fn configure_sentry(sentry_config: SentryConfig, build_info: BuildInfo) {
    let _guard = sentry::init((
        sentry_config.url,
        sentry::ClientOptions {
            traces_sample_rate: sentry_config.sample_rate,
            debug: sentry_config.debug,
            ..sentry::ClientOptions::default()
        }
        ));
    global::set_text_map_propagator(sentry_opentelemetry::SentryPropagator::new());
    let tracer_provider = SdkTracerProvider::builder()
        .with_span_processor(sentry_opentelemetry::SentrySpanProcessor::new())
        .build();
    global::set_tracer_provider(tracer_provider);
}