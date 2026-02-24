#[cfg(feature = "enterprise")]
use opentelemetry::KeyValue;
#[cfg(feature = "enterprise")]
use opentelemetry_otlp::{WithExportConfig, WithHttpConfig};
#[cfg(feature = "enterprise")]
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use opentelemetry_sdk::trace::SdkTracerProvider;
use std::sync::Arc;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};
use unleash_edge_config::logging::LogFormat;
use unleash_edge_config::otel::{OtelExporterProtocol, TracingMode};
use unleash_edge_types::{BackgroundTask, EdgeResult};

#[derive(Debug, Clone)]
pub struct OtelHolder {
    tracer_provider: SdkTracerProvider,
    meter_provider: SdkMeterProvider,
    logger_provider: SdkLoggerProvider,
}

impl OtelHolder {
    pub fn shutdown(&self) {
        let _ = self.tracer_provider.shutdown();
        let _ = self.meter_provider.shutdown();
        let _ = self.logger_provider.shutdown();
    }
}

fn log_filter() -> EnvFilter {
    EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("info"))
        .unwrap()
}
fn formatting_layer<S>(log_format: LogFormat) -> Box<dyn Layer<S> + Send + Sync + 'static>
where
    S: tracing::Subscriber + for<'a> LookupSpan<'a>,
{
    match log_format {
        LogFormat::Plain => tracing_subscriber::fmt::layer().boxed(),
        LogFormat::Json => tracing_subscriber::fmt::layer().json().boxed(),
        LogFormat::Pretty => tracing_subscriber::fmt::layer().pretty().boxed(),
    }
}

#[cfg(feature = "enterprise")]
fn resource(app_id: String, client_id: String) -> Resource {
    Resource::builder()
        .with_service_name("unleash_edge")
        .with_attribute(KeyValue::new(
            "version",
            unleash_edge_types::build::PKG_VERSION,
        ))
        .with_attribute(KeyValue::new("service_instance_id", app_id))
        .with_attribute(KeyValue::new("service_client_id", client_id))
        .build()
}

#[cfg(feature = "enterprise")]
fn init_otel(
    endpoint: &str,
    mode: &OtelExporterProtocol,
    app_id: String,
    client_id: String,
) -> anyhow::Result<(SdkTracerProvider, SdkMeterProvider, SdkLoggerProvider)> {
    let res = resource(app_id, client_id);
    // --- Traces ----
    let span_exporter = match mode {
        OtelExporterProtocol::Http => opentelemetry_otlp::SpanExporter::builder()
            .with_http()
            .with_endpoint(endpoint)
            .with_compression(opentelemetry_otlp::Compression::Gzip)
            .build(),
        OtelExporterProtocol::Grpc => opentelemetry_otlp::SpanExporter::builder()
            .with_tonic()
            .with_endpoint(endpoint)
            .build(),
    }?;
    let tracer_provider = SdkTracerProvider::builder()
        .with_resource(res.clone())
        .with_batch_exporter(span_exporter)
        .build();

    opentelemetry::global::set_tracer_provider(tracer_provider.clone());

    // --- Metrics ---
    let metric_exporter = match mode {
        OtelExporterProtocol::Http => opentelemetry_otlp::MetricExporter::builder()
            .with_http()
            .with_endpoint(endpoint)
            .with_compression(opentelemetry_otlp::Compression::Gzip)
            .build(),
        OtelExporterProtocol::Grpc => opentelemetry_otlp::MetricExporter::builder()
            .with_tonic()
            .with_endpoint(endpoint)
            .build(),
    }?;
    let meter_provider = SdkMeterProvider::builder()
        .with_resource(res.clone())
        .with_periodic_exporter(metric_exporter)
        .build();
    opentelemetry::global::set_meter_provider(meter_provider.clone());

    // --- Logs ---
    let log_exporter = match mode {
        OtelExporterProtocol::Http => opentelemetry_otlp::LogExporter::builder()
            .with_http()
            .with_endpoint(endpoint)
            .with_compression(opentelemetry_otlp::Compression::Gzip)
            .build(),
        OtelExporterProtocol::Grpc => opentelemetry_otlp::LogExporter::builder()
            .with_tonic()
            .with_endpoint(endpoint)
            .build(),
    }?;
    let logger_provider = SdkLoggerProvider::builder()
        .with_resource(res)
        .with_batch_exporter(log_exporter)
        .build();

    Ok((tracer_provider, meter_provider, logger_provider))
}

#[cfg(feature = "enterprise")]
fn enterprise_tracing(tracing: TracingMode) -> EdgeResult<Option<OtelHolder>> {
    match tracing {
        TracingMode::Otel(config) => {
            let (tracer_provider, meter_provider, logger_provider) = init_otel(
                &config.otel_endpoint_url,
                &config.otel_protocol,
                config.app_id,
                config.client_id.clone(),
            )
            .map_err(|e| unleash_edge_types::errors::EdgeError::TracingInitError(e.to_string()))?;
            let _ = init_tracing_subscriber(&logger_provider, config.log_format);
            Ok(Some(OtelHolder {
                tracer_provider,
                meter_provider,
                logger_provider,
            }))
        }
        TracingMode::Simple(log_format) => simple_logging(log_format),
    }
}

#[allow(unused_variables)]
/// Instantiates exporters for traces, metrics and logs
/// the exporter will read environment variables as specified in (Otel docs)[https://opentelemetry.io/docs/specs/otel/protocol/exporter/]
pub fn init_tracing_and_logging(tracing_opts: TracingMode) -> EdgeResult<Option<OtelHolder>> {
    #[cfg(feature = "enterprise")]
    {
        enterprise_tracing(tracing_opts)
    }
    #[cfg(not(feature = "enterprise"))]
    {
        simple_logging(tracing_opts.log_format)
    }
}

fn simple_logging(log_format: LogFormat) -> EdgeResult<Option<OtelHolder>> {
    init_logging(log_format)
        .map_err(|e| {
            println!("Something went wrong {:?}", e);
            e
        })
        .map(|_| None)
}

#[cfg(feature = "enterprise")]
fn init_tracing_subscriber(
    logger_provider: &SdkLoggerProvider,
    log_format: LogFormat,
) -> EdgeResult<()> {
    let otel_logs_layer =
        opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge::new(logger_provider);
    let tracer = opentelemetry::global::tracer("unleash_edge");
    let otel_traces_layer = tracing_opentelemetry::layer().with_tracer(tracer);
    let _ = tracing_subscriber::registry()
        .with(log_filter())
        .with(otel_traces_layer)
        .with(otel_logs_layer)
        .with(formatting_layer(log_format))
        .try_init();
    Ok(())
}

fn init_logging(log_format: LogFormat) -> EdgeResult<()> {
    let _ = tracing_subscriber::registry()
        .with(formatting_layer(log_format))
        .with(log_filter())
        .try_init();
    Ok(())
}

pub fn shutdown_logging(otel_holder: Arc<Option<OtelHolder>>) -> BackgroundTask {
    Box::pin(async move {
        if let Some(holder) = otel_holder.as_ref() {
            holder.shutdown()
        };
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use unleash_edge_cli::CliArgs;

    #[test]
    fn test_simple_logging_initialization() {
        let args = CliArgs::parse_from([
            "unleash-edge",
            "edge",
            "--upstream-url",
            "http://localhost:3000",
        ]);
        let result = simple_logging(args.log_format.clone().into());
        assert!(result.is_ok());
        let holder = result.unwrap();
        assert!(holder.is_none());
    }

    #[test]
    fn test_init_tracing_and_logging_no_otel() {
        let result = init_tracing_and_logging(TracingMode::Simple(LogFormat::Plain));
        match result {
            Ok(r) => {
                assert!(r.is_none())
            }
            Err(e) => {
                panic!("{e:?}")
            }
        }
    }

    #[test]
    #[cfg(feature = "enterprise")]
    fn test_resource_creation() {
        let app_id = "test-instance".to_string();
        let res = resource(app_id.clone(), "self-hosted-test".into());

        let service_name = res.iter().find(|(k, _)| k.as_str() == "service.name");
        assert!(service_name.is_some());

        let instance_id = res
            .iter()
            .find(|(k, _)| k.as_str() == "service_instance_id");
        assert_eq!(instance_id.unwrap().1.to_string(), app_id);
    }
}
