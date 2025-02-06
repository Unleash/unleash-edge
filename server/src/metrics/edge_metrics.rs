use ahash::{HashMap, HashSet};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::debug;
use ulid::Ulid;
use utoipa::ToSchema;

use crate::types::BuildInfo;

#[derive(Debug, Default, Clone, Copy, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LatencyMetrics {
    pub avg: f64,
    pub count: f64,
    pub p99: f64,
}

impl LatencyMetrics {
    pub fn new() -> Self {
        Self {
            avg: 0.0,
            count: 0.0,
            p99: 0.0,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProcessMetrics {
    pub cpu_usage: f64,
    pub memory_usage: f64,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InstanceTraffic {
    pub get: HashMap<String, LatencyMetrics>,
    pub post: HashMap<String, LatencyMetrics>,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpstreamLatency {
    pub features: LatencyMetrics,
    pub metrics: LatencyMetrics,
    pub edge: LatencyMetrics,
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EdgeInstanceData {
    pub identifier: String,
    pub app_name: String,
    pub region: Option<String>,
    pub edge_version: String,
    pub process_metrics: Option<ProcessMetrics>,
    pub started: DateTime<Utc>,
    pub traffic: InstanceTraffic,
    pub latency_upstream: UpstreamLatency,
    pub connected_streaming_clients: u64,
    pub connected_edges: Vec<EdgeInstanceData>,
}

impl EdgeInstanceData {
    pub fn new(app_name: &str) -> Self {
        let build_info = BuildInfo::default();
        Self {
            identifier: Ulid::new().to_string(),
            app_name: app_name.to_string(),
            region: std::env::var("AWS_REGION").ok(),
            edge_version: build_info.package_version.clone(),
            process_metrics: None,
            started: Utc::now(),
            traffic: InstanceTraffic::default(),
            latency_upstream: UpstreamLatency::default(),
            connected_edges: Vec::new(),
            connected_streaming_clients: 0,
        }
    }

    pub fn add_downstream(&mut self, downstream_edge: EdgeInstanceData) -> Self {
        self.connected_edges.push(downstream_edge);
        self.clone()
    }

    pub fn observe(&self, registry: &prometheus::Registry) -> Self {
        let mut observed = self.clone();
        let mut cpu_seconds = 0;
        let mut resident_memory = 0;
        let mut desired_urls = HashSet::default();
        desired_urls.insert("/api/client/features");
        desired_urls.insert("/api/client/metrics");
        desired_urls.insert("/api/client/metrics/bulk");
        desired_urls.insert("/api/client/metrics/edge");
        desired_urls.insert("/api/frontend");
        let mut get_requests = HashMap::default();
        let mut post_requests = HashMap::default();

        for family in registry.gather().iter() {
            match family.get_name() {
                "http_server_duration_milliseconds" => {
                    family
                        .get_metric()
                        .iter()
                        .filter(|m| {
                            m.has_histogram()
                                && m.get_label().iter().any(|l| {
                                    l.get_name() == "url_path"
                                        && desired_urls.contains(l.get_value())
                                })
                                && m.get_label().iter().any(|l| {
                                    l.get_name() == "http_response_status_code"
                                        && l.get_value() == "200"
                                })
                        })
                        .for_each(|m| {
                            let labels = m.get_label();

                            let path = labels
                                .iter()
                                .find(|l| l.get_name() == "url_path")
                                .unwrap()
                                .get_value();
                            let method = labels
                                .iter()
                                .find(|l| l.get_name() == "http_request_method")
                                .unwrap()
                                .get_value();

                            let latency = if method == "GET" {
                                get_requests
                                    .entry(path.to_string())
                                    .or_insert(LatencyMetrics::new())
                            } else {
                                post_requests
                                    .entry(path.to_string())
                                    .or_insert(LatencyMetrics::new())
                            };
                            let total = m.get_histogram().get_sample_sum();
                            let count = m.get_histogram().get_sample_count() as f64;
                            let p99 = get_percentile(
                                99,
                                m.get_histogram().get_sample_count(),
                                m.get_histogram().get_bucket(),
                            );
                            LatencyMetrics {
                                avg: total / count,
                                count,
                                p99,
                            };
                        });
                }
                "process_cpu_seconds_total" => {
                    if let Some(cpu_second_metric) = family.get_metric().last() {
                        cpu_seconds = cpu_second_metric.get_counter().get_value() as u64;
                    }
                }
                "process_resident_memory_bytes" => {
                    if let Some(resident_memory_metric) = family.get_metric().last() {
                        resident_memory = resident_memory_metric.get_gauge().get_value() as u64;
                    }
                }
                "client_metrics_upload" => {
                    if let Some(metrics_upload_metric) = family.get_metric().last() {
                        let total = metrics_upload_metric.get_histogram().get_sample_sum();
                        let count = metrics_upload_metric.get_histogram().get_sample_count();
                        let p99 = get_percentile(
                            99,
                            count,
                            metrics_upload_metric.get_histogram().get_bucket(),
                        );
                        observed.latency_upstream.metrics = LatencyMetrics {
                            avg: metrics_upload_metric.get_histogram().get_sample_sum()
                                / count as f64,
                            count: count as f64,
                            p99,
                        }
                    }
                }
                "instance_data_upload" => {
                    if let Some(instance_data_upload_metric) = family.get_metric().last() {
                        let count = instance_data_upload_metric
                            .get_histogram()
                            .get_sample_count();
                        let p99 = get_percentile(
                            99,
                            count,
                            instance_data_upload_metric.get_histogram().get_bucket(),
                        );
                        observed.latency_upstream.edge = LatencyMetrics {
                            avg: instance_data_upload_metric.get_histogram().get_sample_sum()
                                / count as f64,
                            count: count as f64,
                            p99,
                        }
                    }
                }
                "client_feature_fetch" => {
                    if let Some(feature_fetch_metric) = family.get_metric().last() {
                        let count = feature_fetch_metric.get_histogram().get_sample_count();
                        let p99 = get_percentile(
                            99,
                            count,
                            feature_fetch_metric.get_histogram().get_bucket(),
                        );
                        observed.latency_upstream.features = LatencyMetrics {
                            avg: feature_fetch_metric.get_histogram().get_sample_sum()
                                / count as f64,
                            count: count as f64,
                            p99,
                        }
                    }
                }
                "connected_streaming_clients" => {
                    if let Some(connected_streaming_clients) = family.get_metric().last() {
                        observed.connected_streaming_clients =
                            connected_streaming_clients.get_gauge().get_value() as u64;
                    }
                }
                _ => {}
            }
        }
        observed.traffic = InstanceTraffic {
            get: get_requests,
            post: post_requests,
        };
        observed.process_metrics = Some(ProcessMetrics {
            cpu_usage: cpu_seconds as f64,
            memory_usage: resident_memory as f64,
        });
        observed
    }
}

fn get_percentile(percentile: u64, count: u64, buckets: &[prometheus::proto::Bucket]) -> f64 {
    let mut total = 0;
    let target = count * percentile / 100;
    for bucket in buckets {
        total += bucket.get_cumulative_count();
        if total >= target {
            return bucket.get_upper_bound();
        }
    }
    0.0
}
