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

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestCount {
    pub count: AtomicU64,
}

impl Clone for RequestCount {
    fn clone(&self) -> Self {
        Self {
            count: AtomicU64::new(self.count.load(Ordering::Relaxed)),
        }
    }
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntervalRequestCount {
    pub interval: [u64; 2],
    pub requests: RequestCount,
}

impl Clone for IntervalRequestCount {
    fn clone(&self) -> Self {
        Self {
            interval: self.interval,
            requests: self.requests.clone(),
        }
    }
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DataPoint {
    pub interval: [u64; 2],
    pub requests: AtomicU64,
}

impl Clone for DataPoint {
    fn clone(&self) -> Self {
        Self {
            interval: self.interval,
            requests: AtomicU64::new(self.requests.load(Ordering::Relaxed)),
        }
    }
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsumptionMetrics {
    pub metered_group: String,
    pub data_points: Vec<DataPoint>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Hash, Eq, PartialEq)]
pub enum ConnectionMetricsType {
    Features,
    Metrics,
}

impl ConnectionMetricsType {
    fn from_endpoint(endpoint: &str) -> Option<Self> {
        if endpoint.contains("/features") || endpoint.contains("/delta") {
            Some(Self::Features)
        } else if endpoint.contains("/metrics") {
            Some(Self::Metrics)
        } else {
            None
        }
    }
}

const DEFAULT_METRICS_INTERVAL: u64 = 60000;
const DEFAULT_FEATURES_INTERVAL: u64 = 15000;
const BUCKET_SIZE_METRICS: u64 = 60000;
const BUCKET_SIZE_FEATURES: u64 = 5000;
const MAX_BUCKET_INTERVAL: u64 = 3600000;

#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq, Deserialize, Serialize)]
pub struct BucketRange {
    start: u64,
    end: u64,
}

impl BucketRange {
    pub fn new(start: u64, end: u64) -> Self {
        assert!(
            start <= end,
            "Bucket start must be less than or equal to end"
        );
        Self { start, end }
    }

    pub fn as_array(&self) -> [u64; 2] {
        [self.start, self.end]
    }

    pub fn from_array(arr: [u64; 2]) -> Self {
        Self::new(arr[0], arr[1])
    }
}

/// Represents a metered group for consumption metrics.
/// Currently only supports "default" but can be extended for future use.
#[derive(Debug, Clone, Hash, Eq, PartialEq, Deserialize, Serialize)]
pub struct MeteredGroup(String);

impl MeteredGroup {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for MeteredGroup {
    fn default() -> Self {
        Self("default".to_string())
    }
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestConsumptionData {
    pub metered_group: String,
    pub requests: AtomicU64,
}

impl Clone for RequestConsumptionData {
    fn clone(&self) -> Self {
        Self {
            metered_group: self.metered_group.clone(),
            requests: AtomicU64::new(self.requests.load(Ordering::Relaxed)),
        }
    }
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionConsumptionData {
    #[serde(skip)]
    features_map: DashMap<[u64; 2], DataPoint>,
    #[serde(skip)]
    metrics_map: DashMap<[u64; 2], DataPoint>,
    #[serde(rename = "features")]
    features: Vec<DataPoint>,
    #[serde(rename = "metrics")]
    metrics: Vec<DataPoint>,
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
    pub connection_consumption_since_last_report: ConnectionConsumptionData,
    pub request_consumption_since_last_report: RequestConsumptionData,
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
            connection_consumption_since_last_report: ConnectionConsumptionData::default(),
            request_consumption_since_last_report: RequestConsumptionData {
                metered_group: "default".to_string(),
                requests: AtomicU64::new(0),
            },
        }
    }

    pub fn clear_time_windowed_metrics(&self) {
        self.requests_since_last_report.clear();
        self.connection_consumption_since_last_report.features_map.clear();
        self.connection_consumption_since_last_report.metrics_map.clear();
        self.request_consumption_since_last_report.requests.store(0, Ordering::SeqCst);
    }

    pub fn observe_request(&self, http_target: &str, status_code: u16) {
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

    pub fn get_interval_bucket(endpoint: &str, interval_ms: Option<u64>) -> BucketRange {
        if endpoint.is_empty() {
            return BucketRange::new(0, DEFAULT_FEATURES_INTERVAL);
        }

        let interval = interval_ms.unwrap_or(if endpoint.ends_with("/metrics") {
            DEFAULT_METRICS_INTERVAL
        } else {
            DEFAULT_FEATURES_INTERVAL
        });

        if interval > MAX_BUCKET_INTERVAL {
            return BucketRange::new(MAX_BUCKET_INTERVAL, u64::MAX);
        }

        if endpoint.ends_with("/metrics") {
            Self::get_metrics_bucket(interval)
        } else {
            Self::get_features_bucket(interval)
        }
    }

    fn get_metrics_bucket(interval: u64) -> BucketRange {
        if interval <= DEFAULT_METRICS_INTERVAL {
            BucketRange::new(0, DEFAULT_METRICS_INTERVAL)
        } else {
            let bucket_start = (interval / BUCKET_SIZE_METRICS) * BUCKET_SIZE_METRICS;
            BucketRange::new(bucket_start, bucket_start + BUCKET_SIZE_METRICS)
        }
    }

    fn get_features_bucket(interval: u64) -> BucketRange {
        if interval <= DEFAULT_FEATURES_INTERVAL {
            BucketRange::new(0, DEFAULT_FEATURES_INTERVAL)
        } else {
            let bucket_start = ((interval - DEFAULT_FEATURES_INTERVAL) / BUCKET_SIZE_FEATURES)
                * BUCKET_SIZE_FEATURES
                + DEFAULT_FEATURES_INTERVAL;
            BucketRange::new(bucket_start, bucket_start + BUCKET_SIZE_FEATURES)
        }
    }

    pub fn observe_connection_consumption(&self, endpoint: &str, interval: Option<u64>) {
        let bucket = Self::get_interval_bucket(endpoint, interval);
        if let Some(metrics_type) = ConnectionMetricsType::from_endpoint(endpoint) {
            let data_points_map = match metrics_type {
                ConnectionMetricsType::Features => &self.connection_consumption_since_last_report.features_map,
                ConnectionMetricsType::Metrics => &self.connection_consumption_since_last_report.metrics_map,
            };

            data_points_map.entry(bucket.as_array())
                .or_insert_with(|| DataPoint {
                    interval: bucket.as_array(),
                    requests: AtomicU64::new(0),
                })
                .requests
                .fetch_add(1, Ordering::SeqCst);
        }
    }

    pub fn observe_request_consumption(&self) {
        self.request_consumption_since_last_report.requests.fetch_add(1, Ordering::SeqCst);
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
                crate::metrics::actix_web_prometheus_metrics::HTTP_REQUESTS_DURATION => {
                    family
                        .get_metric()
                        .iter()
                        .filter(|m| {
                            m.has_histogram() && m.get_label().iter().any(|l| {
                                l.get_name()
                                    == crate::metrics::actix_web_prometheus_metrics::ENDPOINT_LABEL
                                    && DESIRED_URLS
                                        .iter()
                                        .any(|desired| l.get_value().ends_with(desired))
                            }) && m.get_label().iter().any(|l| {
                                l.get_name()
                                    == crate::metrics::actix_web_prometheus_metrics::STATUS_LABEL
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
                                .find(|l| l.get_name() == crate::metrics::actix_web_prometheus_metrics::ENDPOINT_LABEL)
                                .unwrap()
                                .get_value()
                                .strip_prefix(base_path)
                                .unwrap();
                            let method = labels
                                .iter()
                                .find(|l| l.get_name() == crate::metrics::actix_web_prometheus_metrics::METHOD_LABEL)
                                .unwrap()
                                .get_value();
                            let status = labels
                                .iter()
                                .find(|l| l.get_name() == crate::metrics::actix_web_prometheus_metrics::STATUS_LABEL)
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
                            let total = m.get_histogram().get_sample_sum(); // convert to ms
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

    pub fn get_connection_consumption_data(&self) -> ConnectionConsumptionData {
        let mut data = ConnectionConsumptionData::default();
        
        // Clone the data points with their current values
        for entry in self.connection_consumption_since_last_report.features_map.iter() {
            data.features_map.insert(
                *entry.key(),
                DataPoint {
                    interval: *entry.key(),
                    requests: AtomicU64::new(entry.value().requests.load(Ordering::Relaxed)),
                },
            );
            data.features.push(DataPoint {
                interval: *entry.key(),
                requests: AtomicU64::new(entry.value().requests.load(Ordering::Relaxed)),
            });
        }

        for entry in self.connection_consumption_since_last_report.metrics_map.iter() {
            data.metrics_map.insert(
                *entry.key(),
                DataPoint {
                    interval: *entry.key(),
                    requests: AtomicU64::new(entry.value().requests.load(Ordering::Relaxed)),
                },
            );
            data.metrics.push(DataPoint {
                interval: *entry.key(),
                requests: AtomicU64::new(entry.value().requests.load(Ordering::Relaxed)),
            });
        }

        data
    }

    pub fn get_request_consumption_data(&self) -> Vec<RequestConsumptionData> {
        vec![RequestConsumptionData {
            metered_group: "default".to_string(),
            requests: AtomicU64::new(self.request_consumption_since_last_report.requests.load(Ordering::Relaxed)),
        }]
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
    use super::*;

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

    #[test]
    pub fn can_observe_and_clear_consumption_metrics() {
        let instance = EdgeInstanceData::new("test-app");

        // Test features endpoint with different intervals
        instance.observe_connection_consumption("/api/client/features", None);
        instance.observe_connection_consumption("/api/client/features", Some(15000));

        // Test metrics endpoint with different intervals
        instance.observe_connection_consumption("/api/client/metrics", None);
        instance.observe_connection_consumption("/api/client/metrics", Some(60000));

        // Test frontend consumption
        instance.observe_request_consumption();
        instance.observe_request_consumption();

        // Verify features endpoint metrics in internal counters
        if let Some(features_map) = instance
            .connection_consumption_since_last_report.features.iter().find(|dp| dp.interval == [0, 15000])
        {
            assert_eq!(features_map.requests.load(Ordering::Relaxed), 2);
        }

        // Verify metrics endpoint metrics in internal counters
        if let Some(metrics_map) = instance
            .connection_consumption_since_last_report.metrics.iter().find(|dp| dp.interval == [0, 60000])
        {
            assert_eq!(metrics_map.requests.load(Ordering::Relaxed), 2);
        }

        // Verify frontend consumption in internal counters
        assert_eq!(instance.request_consumption_since_last_report.requests.load(Ordering::Relaxed), 2);

        // Verify serialized connection consumption data
        let connection_data = instance.get_connection_consumption_data();
        
        // Verify features data points
        assert_eq!(connection_data.features.len(), 1);
        assert_eq!(connection_data.features[0].interval, [0, 15000]);
        assert_eq!(connection_data.features[0].requests.load(Ordering::Relaxed), 2);

        // Verify metrics data points
        assert_eq!(connection_data.metrics.len(), 1);
        assert_eq!(connection_data.metrics[0].interval, [0, 60000]);
        assert_eq!(connection_data.metrics[0].requests.load(Ordering::Relaxed), 2);

        // Verify serialized request consumption data
        let request_data = instance.get_request_consumption_data();
        assert_eq!(request_data.len(), 1);
        assert_eq!(request_data[0].metered_group, "default");
        assert_eq!(request_data[0].requests.load(Ordering::Relaxed), 2);

        // Verify clearing metrics
        instance.clear_time_windowed_metrics();
        assert!(instance.connection_consumption_since_last_report.features.is_empty());
        assert!(instance.connection_consumption_since_last_report.metrics.is_empty());
        assert_eq!(instance.request_consumption_since_last_report.requests.load(Ordering::Relaxed), 0);

        // Verify cleared serialized data
        let cleared_connection_data = instance.get_connection_consumption_data();
        assert!(cleared_connection_data.features.is_empty());
        assert!(cleared_connection_data.metrics.is_empty());

        let cleared_request_data = instance.get_request_consumption_data();
        assert_eq!(cleared_request_data.len(), 1);
        assert_eq!(cleared_request_data[0].metered_group, "default");
        assert_eq!(cleared_request_data[0].requests.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_bucket_boundaries() {
        // Test features endpoint bucket boundaries
        assert_eq!(
            EdgeInstanceData::get_interval_bucket("/api/client/features", None),
            BucketRange::new(0, 15000)
        );
        assert_eq!(
            EdgeInstanceData::get_interval_bucket("/api/client/features", Some(0)),
            BucketRange::new(0, 15000)
        );
        assert_eq!(
            EdgeInstanceData::get_interval_bucket("/api/client/features", Some(14999)),
            BucketRange::new(0, 15000)
        );
        assert_eq!(
            EdgeInstanceData::get_interval_bucket("/api/client/features", Some(15000)),
            BucketRange::new(0, 15000)
        );
        assert_eq!(
            EdgeInstanceData::get_interval_bucket("/api/client/features", Some(15001)),
            BucketRange::new(15000, 20000)
        );
        assert_eq!(
            EdgeInstanceData::get_interval_bucket("/api/client/features", Some(19999)),
            BucketRange::new(15000, 20000)
        );
        assert_eq!(
            EdgeInstanceData::get_interval_bucket("/api/client/features", Some(20000)),
            BucketRange::new(20000, 25000)
        );

        // Test metrics endpoint bucket boundaries
        assert_eq!(
            EdgeInstanceData::get_interval_bucket("/api/client/metrics", None),
            BucketRange::new(0, 60000)
        );
        assert_eq!(
            EdgeInstanceData::get_interval_bucket("/api/client/metrics", Some(0)),
            BucketRange::new(0, 60000)
        );
        assert_eq!(
            EdgeInstanceData::get_interval_bucket("/api/client/metrics", Some(59999)),
            BucketRange::new(0, 60000)
        );
        assert_eq!(
            EdgeInstanceData::get_interval_bucket("/api/client/metrics", Some(60000)),
            BucketRange::new(0, 60000)
        );
        assert_eq!(
            EdgeInstanceData::get_interval_bucket("/api/client/metrics", Some(60001)),
            BucketRange::new(60000, 120000)
        );
        assert_eq!(
            EdgeInstanceData::get_interval_bucket("/api/client/metrics", Some(119999)),
            BucketRange::new(60000, 120000)
        );
        assert_eq!(
            EdgeInstanceData::get_interval_bucket("/api/client/metrics", Some(120000)),
            BucketRange::new(120000, 180000)
        );

        // Test maximum bucket
        assert_eq!(
            EdgeInstanceData::get_interval_bucket("/api/client/features", Some(3600001)),
            BucketRange::new(3600000, u64::MAX)
        );
        assert_eq!(
            EdgeInstanceData::get_interval_bucket("/api/client/metrics", Some(3600001)),
            BucketRange::new(3600000, u64::MAX)
        );
    }

    #[test]
    fn test_endpoint_matching() {
        assert_eq!(
            ConnectionMetricsType::from_endpoint("/api/client/features"),
            Some(ConnectionMetricsType::Features)
        );
        assert_eq!(
            ConnectionMetricsType::from_endpoint("/api/client/delta"),
            Some(ConnectionMetricsType::Features)
        );
        assert_eq!(
            ConnectionMetricsType::from_endpoint("/api/client/metrics"),
            Some(ConnectionMetricsType::Metrics)
        );
        assert_eq!(
            ConnectionMetricsType::from_endpoint("/api/client/metrics/bulk"),
            Some(ConnectionMetricsType::Metrics)
        );
        assert_eq!(
            ConnectionMetricsType::from_endpoint("/api/client/metrics/edge"),
            Some(ConnectionMetricsType::Metrics)
        );
        assert_eq!(
            ConnectionMetricsType::from_endpoint("/api/client/other"),
            None
        );
    }
}
