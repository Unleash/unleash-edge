use crate::logging::LogFormat;
use unleash_edge_cli::CliArgs;

#[derive(Debug, Clone)]
pub struct OtelConfig {
    pub app_id: String,
    pub client_id: String,
    pub log_format: LogFormat,
    pub otel_endpoint_url: String,
    pub otel_protocol: OtelExporterProtocol,
}

#[derive(Debug, Clone)]
pub enum TracingMode {
    Otel(OtelConfig),
    Simple(LogFormat),
}

#[derive(Debug, Clone)]
pub enum OtelExporterProtocol {
    Grpc,
    Http,
}

impl From<unleash_edge_cli::OtelExporterProtocol> for OtelExporterProtocol {
    fn from(value: unleash_edge_cli::OtelExporterProtocol) -> Self {
        match value {
            unleash_edge_cli::OtelExporterProtocol::Grpc => OtelExporterProtocol::Grpc,
            unleash_edge_cli::OtelExporterProtocol::Http => OtelExporterProtocol::Http,
        }
    }
}

impl From<&CliArgs> for TracingMode {
    fn from(args: &CliArgs) -> Self {
        if let Some(endpoint_url) = args.otel_config.otel_exporter_otlp_endpoint.as_ref() {
            TracingMode::Otel(OtelConfig {
                app_id: args.instance_id.clone(),
                client_id: args.client_id.clone(),
                log_format: LogFormat::Plain,
                otel_endpoint_url: endpoint_url.clone(),
                otel_protocol: args.otel_config.otel_exporter_otlp_protocol.clone().into(),
            })
        } else {
            TracingMode::Simple(args.log_format.clone().into())
        }
    }
}
