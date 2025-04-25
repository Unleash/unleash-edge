use std::collections::HashMap;

use crate::http::background_send_metrics;
#[cfg(target_os = "linux")]
use prometheus::process_collector::ProcessCollector;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{EnvFilter, Registry};
use unleash_edge_client_metrics::{FEATURE_TOGGLE_USAGE_TOTAL, METRICS_SIZE_HISTOGRAM};
use unleash_edge_http_metrics::actix_web_prometheus_metrics::{
    PrometheusMetrics, PrometheusMetricsBuilder,
};
use unleash_edge_metrics::EdgeInstanceData;
use unleash_edge_types::build::{PKG_VERSION, SHORT_COMMIT};
use unleash_edge_types::cli::LogFormat;

fn instantiate_tracing_and_logging(log_format: &LogFormat) {
    let env_filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("info"))
        .unwrap();
    match log_format {
        LogFormat::Plain => {
            let logger = tracing_subscriber::fmt::layer();
            let collector = Registry::default().with(logger).with(env_filter);
            tracing::subscriber::set_global_default(collector).unwrap();
        }
        LogFormat::Json => {
            let logger = tracing_subscriber::fmt::layer().json();
            let collector = Registry::default().with(logger).with(env_filter);
            tracing::subscriber::set_global_default(collector).unwrap();
        }
        LogFormat::Pretty => {
            let logger = tracing_subscriber::fmt::layer().pretty();
            let collector = Registry::default().with(logger).with(env_filter);
            tracing::subscriber::set_global_default(collector).unwrap();
        }
    };
}

pub fn instantiate(
    registry: Option<prometheus::Registry>,
    disable_metrics_endpoint: bool,
    log_format: &LogFormat,
    instance_data: &EdgeInstanceData,
) -> PrometheusMetrics {
    instantiate_tracing_and_logging(log_format);
    let registry = registry.unwrap_or_else(instantiate_registry);
    register_custom_metrics(&registry);
    instantiate_prometheus_metrics_handler(registry, disable_metrics_endpoint, instance_data)
}

fn instantiate_prometheus_metrics_handler(
    registry: prometheus::Registry,
    disable_metrics_endpoint: bool,
    instance_data: &EdgeInstanceData,
) -> PrometheusMetrics {
    let mut extra_labels = HashMap::<String, String>::new();
    extra_labels.insert("edge_version".to_string(), PKG_VERSION.to_string());
    extra_labels.insert("edge_githash".to_string(), SHORT_COMMIT.to_string());
    extra_labels.insert("app_name".to_string(), instance_data.app_name.clone());
    extra_labels.insert("instance_id".to_string(), instance_data.identifier.clone());

    PrometheusMetricsBuilder::new("")
        .endpoint("/internal-backstage/metrics")
        .const_labels(extra_labels)
        .registry(registry)
        .exclude("/favicon.ico")
        .disable_metrics_endpoint(disable_metrics_endpoint)
        .build()
        .unwrap()
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
        .register(Box::new(METRICS_SIZE_HISTOGRAM.clone()))
        .unwrap();
    registry
        .register(Box::new(
            background_send_metrics::METRICS_UPSTREAM_CLIENT_BULK.clone(),
        ))
        .unwrap();
    registry
        .register(Box::new(
            background_send_metrics::METRICS_UPSTREAM_OUTDATED.clone(),
        ))
        .unwrap();
    registry
        .register(Box::new(
            background_send_metrics::METRICS_INTERVAL_BETWEEN_SEND.clone(),
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
    registry
        .register(Box::new(
            crate::http::unleash_client::UPSTREAM_VERSION.clone(),
        ))
        .unwrap();
    registry
        .register(Box::new(FEATURE_TOGGLE_USAGE_TOTAL.clone()))
        .unwrap();
    registry
        .register(Box::new(
            crate::http::broadcaster::CONNECTED_STREAMING_CLIENTS.clone(),
        ))
        .unwrap();
    registry
        .register(Box::new(
            crate::http::unleash_client::METRICS_UPLOAD.clone(),
        ))
        .unwrap();
    registry
        .register(Box::new(
            crate::http::unleash_client::INSTANCE_DATA_UPLOAD.clone(),
        ))
        .unwrap();
}

#[cfg(test)]
pub fn test_instantiate_without_tracing_and_logging(
    registry: Option<prometheus::Registry>,
) -> PrometheusMetrics {
    let registry = registry.unwrap_or_else(instantiate_registry);
    register_custom_metrics(&registry);
    instantiate_prometheus_metrics_handler(registry, false, &EdgeInstanceData::new("test app"))
}
