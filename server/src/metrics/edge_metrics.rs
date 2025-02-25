use std::sync::atomic::{AtomicU64, Ordering};

use ahash::HashMap;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
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

pub const DESIRED_URLS: [&str; 6] = [
    "/api/client/features",
    "/api/client/metrics",
    "/api/client/metrics/bulk",
    "/api/client/metrics/edge",
    "/api/frontend",
    "/api/proxy",
];

#[derive(Debug, Default, Clone, Copy, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProcessMetrics {
    pub cpu_usage: f64,
    pub memory_usage: f64,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InstanceTraffic {
    pub cached_responses: HashMap<String, LatencyMetrics>,
    pub get: HashMap<String, LatencyMetrics>,
    pub post: HashMap<String, LatencyMetrics>,
    pub access_denied: HashMap<String, LatencyMetrics>,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpstreamLatency {
    pub features: LatencyMetrics,
    pub metrics: LatencyMetrics,
    pub edge: LatencyMetrics,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestStats {
    pub requests_200: AtomicU64,
    pub requests_304: AtomicU64,
}

impl Clone for RequestStats {
    fn clone(&self) -> Self {
        Self {
            requests_200: AtomicU64::new(self.requests_200.load(Ordering::Relaxed)),
            requests_304: AtomicU64::new(self.requests_304.load(Ordering::Relaxed)),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
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
    pub requests_since_last_report: DashMap<String, RequestStats>,
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
            connected_edges: vec![],
            connected_streaming_clients: 0,
            requests_since_last_report: DashMap::default(),
        }
    }

    pub fn observe_request(&self, http_target: &str, status_code: i64) {
        match status_code {
            200 | 202 | 204 => {
                self.requests_since_last_report
                    .entry(http_target.to_string())
                    .or_default()
                    .requests_200
                    .fetch_add(1, Ordering::SeqCst);
            }
            304 => {
                self.requests_since_last_report
                    .entry(http_target.to_string())
                    .or_default()
                    .requests_304
                    .fetch_add(1, Ordering::SeqCst);
            }
            _ => {}
        }
    }

    pub fn observe(
        &self,
        registry: &prometheus::Registry,
        connected_instances: Vec<EdgeInstanceData>,
        base_path: &str,
    ) -> Self {
        let mut observed = self.clone();
        let mut cpu_seconds = 0;
        let mut resident_memory = 0;
        let mut get_requests = HashMap::default();
        let mut post_requests = HashMap::default();
        let mut access_denied = HashMap::default();
        let mut no_change = HashMap::default();

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
                                        && DESIRED_URLS
                                            .iter()
                                            .any(|desired| l.get_value().ends_with(desired))
                                })
                                && m.get_label().iter().any(|l| {
                                    l.get_name() == "http_response_status_code"
                                        && l.get_value() == "200"
                                        || l.get_value() == "202"
                                        || l.get_value() == "304"
                                        || l.get_value() == "403"
                                })
                        })
                        .for_each(|m| {
                            let labels = m.get_label();

                            let path = labels
                                .iter()
                                .find(|l| l.get_name() == "url_path")
                                .unwrap()
                                .get_value()
                                .strip_prefix(base_path)
                                .unwrap();
                            let method = labels
                                .iter()
                                .find(|l| l.get_name() == "http_request_method")
                                .unwrap()
                                .get_value();
                            let status = labels
                                .iter()
                                .find(|l| l.get_name() == "http_response_status_code")
                                .unwrap()
                                .get_value();
                            let latency = match status {
                                "200" | "202" => {
                                    if method == "GET" {
                                        get_requests
                                            .entry(path.to_string())
                                            .or_insert(LatencyMetrics::default())
                                    } else {
                                        post_requests
                                            .entry(path.to_string())
                                            .or_insert(LatencyMetrics::default())
                                    }
                                }
                                "304" => no_change
                                    .entry(path.to_string())
                                    .or_insert(LatencyMetrics::default()),
                                _ => access_denied
                                    .entry(path.to_string())
                                    .or_insert(LatencyMetrics::default()),
                            };
                            let total = m.get_histogram().get_sample_sum() * 1000.0; // convert to ms
                            let count = m.get_histogram().get_sample_count() as f64;
                            let p99 = get_percentile(
                                99,
                                m.get_histogram().get_sample_count(),
                                m.get_histogram().get_bucket(),
                            );
                            *latency = LatencyMetrics {
                                avg: if count == 0.0 {
                                    0.0
                                } else {
                                    round_to_3_decimals(total / count)
                                },
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
                        let count = metrics_upload_metric.get_histogram().get_sample_count();
                        let p99 = get_percentile(
                            99,
                            count,
                            metrics_upload_metric.get_histogram().get_bucket(),
                        );
                        observed.latency_upstream.metrics = LatencyMetrics {
                            avg: round_to_3_decimals(
                                metrics_upload_metric.get_histogram().get_sample_sum()
                                    / count as f64,
                            ),
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
                            avg: round_to_3_decimals(
                                instance_data_upload_metric.get_histogram().get_sample_sum()
                                    / count as f64,
                            ),
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
                            avg: round_to_3_decimals(
                                feature_fetch_metric.get_histogram().get_sample_sum()
                                    / count as f64,
                            ),
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
            access_denied,
            cached_responses: no_change,
        };
        observed.process_metrics = Some(ProcessMetrics {
            cpu_usage: cpu_seconds as f64,
            memory_usage: resident_memory as f64,
        });
        for connected_instance in connected_instances {
            observed.connected_edges.push(connected_instance.clone());
        }
        observed
    }
}

fn get_percentile(percentile: u64, count: u64, buckets: &[prometheus::proto::Bucket]) -> f64 {
    let target = (percentile as f64 / 100.0) * count as f64;
    let mut previous_upper_bound = 0.0;
    let mut previous_count = 0;
    for bucket in buckets {
        if bucket.get_cumulative_count() as f64 >= target {
            let nth_count = bucket.get_cumulative_count() - previous_count;
            let observation_in_range = target - previous_count as f64;
            return round_to_3_decimals(
                previous_upper_bound
                    + ((observation_in_range / nth_count as f64)
                        * (bucket.get_upper_bound() - previous_upper_bound)),
            );
        }
        previous_upper_bound = bucket.get_upper_bound();
        previous_count = bucket.get_cumulative_count();
    }
    0.0
}

fn round_to_3_decimals(number: f64) -> f64 {
    (number * 1000.0).round() / 1000.0
}

#[cfg(test)]
mod tests {

    #[test]
    pub fn can_find_p99_of_a_range() {
        let mut one_ms = prometheus::proto::Bucket::new();
        one_ms.set_cumulative_count(1000);
        one_ms.set_upper_bound(1.0);
        let mut five_ms = prometheus::proto::Bucket::new();
        five_ms.set_cumulative_count(2000);
        five_ms.set_upper_bound(5.0);
        let mut ten_ms = prometheus::proto::Bucket::new();
        ten_ms.set_cumulative_count(3000);
        ten_ms.set_upper_bound(10.0);
        let mut twenty_ms = prometheus::proto::Bucket::new();
        twenty_ms.set_cumulative_count(4000);
        twenty_ms.set_upper_bound(20.0);
        let mut fifty_ms = prometheus::proto::Bucket::new();
        fifty_ms.set_cumulative_count(5000);
        fifty_ms.set_upper_bound(50.0);
        let buckets = vec![one_ms, five_ms, ten_ms, twenty_ms, fifty_ms];
        let result = super::get_percentile(99, 5000, &buckets);
        assert_eq!(result, 48.5);
    }

    #[test]
    pub fn can_find_p50_of_a_range() {
        let mut one_ms = prometheus::proto::Bucket::new();
        one_ms.set_cumulative_count(1000);
        one_ms.set_upper_bound(1.0);
        let mut five_ms = prometheus::proto::Bucket::new();
        five_ms.set_cumulative_count(2000);
        five_ms.set_upper_bound(5.0);
        let mut ten_ms = prometheus::proto::Bucket::new();
        ten_ms.set_cumulative_count(3000);
        ten_ms.set_upper_bound(10.0);
        let mut twenty_ms = prometheus::proto::Bucket::new();
        twenty_ms.set_cumulative_count(4000);
        twenty_ms.set_upper_bound(20.0);
        let mut fifty_ms = prometheus::proto::Bucket::new();
        fifty_ms.set_cumulative_count(5000);
        fifty_ms.set_upper_bound(50.0);
        let buckets = vec![one_ms, five_ms, ten_ms, twenty_ms, fifty_ms];
        let result = super::get_percentile(50, 5000, &buckets);
        assert_eq!(result, 7.5);
    }
}
