use ahash::HashMap;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
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
pub struct ConsumptionGroup {
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

#[derive(Debug, Default)]
pub struct ConnectionConsumptionData {
    features_map: DashMap<[u64; 2], DataPoint>,
    metrics_map: DashMap<[u64; 2], DataPoint>,
}

impl Clone for ConnectionConsumptionData {
    fn clone(&self) -> Self {
        Self {
            features_map: self.features_map.clone(),
            metrics_map: self.metrics_map.clone(),
        }
    }
}

impl Serialize for ConnectionConsumptionData {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("ConnectionConsumptionData", 2)?;

        // Serialize features
        let mut features_data_points = Vec::new();
        for entry in self.features_map.iter() {
            features_data_points.push(DataPoint {
                interval: *entry.key(),
                requests: AtomicU64::new(entry.value().requests.load(Ordering::Relaxed)),
            });
        }
        let features = vec![ConsumptionGroup {
            metered_group: "default".to_string(),
            data_points: features_data_points,
        }];

        // Serialize metrics
        let mut metrics_data_points = Vec::new();
        for entry in self.metrics_map.iter() {
            metrics_data_points.push(DataPoint {
                interval: *entry.key(),
                requests: AtomicU64::new(entry.value().requests.load(Ordering::Relaxed)),
            });
        }
        let metrics = vec![ConsumptionGroup {
            metered_group: "default".to_string(),
            data_points: metrics_data_points,
        }];

        state.serialize_field("features", &features)?;
        state.serialize_field("metrics", &metrics)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for ConnectionConsumptionData {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Helper {
            features: Vec<ConsumptionGroup>,
            metrics: Vec<ConsumptionGroup>,
        }

        let helper = Helper::deserialize(deserializer)?;
        let data = ConnectionConsumptionData::default();

        // Convert features groups to map entries
        for group in helper.features {
            for point in group.data_points {
                data.features_map.insert(point.interval, point);
            }
        }

        // Convert metrics groups to map entries
        for group in helper.metrics {
            for point in group.data_points {
                data.metrics_map.insert(point.interval, point);
            }
        }

        Ok(data)
    }
}

impl ConnectionConsumptionData {
    pub fn reset(&self) {
        self.features_map.clear();
        self.metrics_map.clear();
    }
}

#[derive(Debug, Default)]
pub struct RequestConsumptionData {
    metered_groups: DashMap<String, AtomicU64>,
}

impl Clone for RequestConsumptionData {
    fn clone(&self) -> Self {
        let new_map = DashMap::new();
        for entry in self.metered_groups.iter() {
            new_map.insert(
                entry.key().clone(),
                AtomicU64::new(entry.value().load(Ordering::Relaxed)),
            );
        }
        Self {
            metered_groups: new_map,
        }
    }
}

impl Serialize for RequestConsumptionData {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeSeq;
        let mut seq = serializer.serialize_seq(Some(self.metered_groups.len()))?;
        for entry in self.metered_groups.iter() {
            seq.serialize_element(&serde_json::json!({
                "meteredGroup": entry.key(),
                "requests": entry.value().load(Ordering::Relaxed)
            }))?;
        }
        seq.end()
    }
}

impl<'de> Deserialize<'de> for RequestConsumptionData {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct GroupData {
            metered_group: String,
            requests: u64,
        }

        let groups: Vec<GroupData> = Vec::deserialize(deserializer)?;
        let metered_groups = DashMap::new();
        for group in groups {
            metered_groups.insert(group.metered_group, AtomicU64::new(group.requests));
        }
        Ok(Self { metered_groups })
    }
}

impl RequestConsumptionData {
    pub fn get_requests(&self, metered_group: &str) -> u64 {
        self.metered_groups
            .get(metered_group)
            .map(|v| v.load(Ordering::Relaxed))
            .unwrap_or(0)
    }

    pub fn increment_requests(&self, metered_group: &str) {
        let entry = self.metered_groups.entry(metered_group.to_string());
        match entry {
            dashmap::mapref::entry::Entry::Occupied(mut e) => {
                e.get_mut().fetch_add(1, Ordering::Relaxed);
            }
            dashmap::mapref::entry::Entry::Vacant(e) => {
                e.insert(AtomicU64::new(1));
            }
        }
    }

    pub fn reset(&self) {
        for mut entry in self.metered_groups.iter_mut() {
            entry.value_mut().store(0, Ordering::Relaxed);
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
    pub connection_consumption_since_last_report: ConnectionConsumptionData,
    pub request_consumption_since_last_report: RequestConsumptionData,
}

impl EdgeInstanceData {
    pub fn new(app_name: &str, identifier: &Ulid) -> Self {
        let build_info = BuildInfo::default();
        Self {
            identifier: identifier.to_string(),
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
                metered_groups: DashMap::new(),
            },
        }
    }

    pub fn clear_time_windowed_metrics(&self) {
        self.requests_since_last_report.clear();
        self.connection_consumption_since_last_report.reset();
        self.request_consumption_since_last_report.reset();
    }

    pub fn observe_request_consumption(&self) {
        self.request_consumption_since_last_report
            .increment_requests("default");
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

    pub fn get_interval_bucket(endpoint: &str, interval_ms: Option<u64>) -> std::ops::Range<u64> {
        if endpoint.is_empty() {
            return 0..DEFAULT_FEATURES_INTERVAL;
        }

        let interval = interval_ms.unwrap_or(if endpoint.ends_with("/metrics") {
            DEFAULT_METRICS_INTERVAL
        } else {
            DEFAULT_FEATURES_INTERVAL
        });

        // For intervals greater than 1 hour, use [1h, 1h] range
        if interval > MAX_BUCKET_INTERVAL {
            return MAX_BUCKET_INTERVAL..MAX_BUCKET_INTERVAL;
        }

        if endpoint.ends_with("/metrics") {
            Self::get_metrics_bucket(interval)
        } else {
            Self::get_features_bucket(interval)
        }
    }

    fn get_metrics_bucket(interval: u64) -> std::ops::Range<u64> {
        if interval <= DEFAULT_METRICS_INTERVAL {
            0..DEFAULT_METRICS_INTERVAL
        } else {
            let bucket_start = (interval / BUCKET_SIZE_METRICS) * BUCKET_SIZE_METRICS;
            bucket_start..(bucket_start + BUCKET_SIZE_METRICS)
        }
    }

    fn get_features_bucket(interval: u64) -> std::ops::Range<u64> {
        if interval <= DEFAULT_FEATURES_INTERVAL {
            0..DEFAULT_FEATURES_INTERVAL
        } else {
            let bucket_start = ((interval - DEFAULT_FEATURES_INTERVAL) / BUCKET_SIZE_FEATURES)
                * BUCKET_SIZE_FEATURES
                + DEFAULT_FEATURES_INTERVAL;
            bucket_start..(bucket_start + BUCKET_SIZE_FEATURES)
        }
    }

    pub fn observe_connection_consumption(&self, endpoint: &str, interval: Option<u64>) {
        let bucket = Self::get_interval_bucket(endpoint, interval);
        if let Some(metrics_type) = ConnectionMetricsType::from_endpoint(endpoint) {
            match metrics_type {
                ConnectionMetricsType::Features => {
                    self.connection_consumption_since_last_report
                        .features_map
                        .entry([bucket.start, bucket.end])
                        .or_insert_with(|| DataPoint {
                            interval: [bucket.start, bucket.end],
                            requests: AtomicU64::new(0),
                        })
                        .requests
                        .fetch_add(1, Ordering::SeqCst);
                }
                ConnectionMetricsType::Metrics => {
                    self.connection_consumption_since_last_report
                        .metrics_map
                        .entry([bucket.start, bucket.end])
                        .or_insert_with(|| DataPoint {
                            interval: [bucket.start, bucket.end],
                            requests: AtomicU64::new(0),
                        })
                        .requests
                        .fetch_add(1, Ordering::SeqCst);
                }
            }
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
            match family.name() {
                crate::metrics::actix_web_prometheus_metrics::HTTP_REQUESTS_DURATION => {
                    family
                        .get_metric()
                        .iter()
                        .filter(|m| {
                            m.get_label().iter().any(|l| {
                                l.name()
                                    == crate::metrics::actix_web_prometheus_metrics::ENDPOINT_LABEL
                                    && DESIRED_URLS
                                        .iter()
                                        .any(|desired| l.value().ends_with(desired))
                            }) && m.get_label().iter().any(|l| {
                                l.name()
                                    == crate::metrics::actix_web_prometheus_metrics::STATUS_LABEL
                                    && (l.value() == "200"
                                        || l.value() == "202"
                                        || l.value() == "304"
                                        || l.value() == "403")
                            })
                        })
                        .for_each(|m| {
                            let labels = m.get_label();
                            let path = labels
                                .iter()
                                .find(|l| l.name() == crate::metrics::actix_web_prometheus_metrics::ENDPOINT_LABEL)
                                .unwrap()
                                .value()
                                .strip_prefix(base_path)
                                .unwrap();
                            let method = labels
                                .iter()
                                .find(|l| l.name() == crate::metrics::actix_web_prometheus_metrics::METHOD_LABEL)
                                .unwrap()
                                .value();
                            let status = labels
                                .iter()
                                .find(|l| l.name() == crate::metrics::actix_web_prometheus_metrics::STATUS_LABEL)
                                .unwrap()
                                .value();
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
                        cpu_seconds = cpu_second_metric.get_counter().value() as u64;
                    }
                }
                "process_resident_memory_bytes" => {
                    if let Some(resident_memory_metric) = family.get_metric().last() {
                        resident_memory = resident_memory_metric.get_gauge().value() as u64;
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
                            connected_streaming_clients.get_gauge().value() as u64;
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
        if bucket.cumulative_count() as f64 >= target {
            let nth_count = bucket.cumulative_count() - previous_count;
            let observation_in_range = target - previous_count as f64;
            return round_to_3_decimals(
                previous_upper_bound
                    + ((observation_in_range / nth_count as f64)
                        * (bucket.upper_bound() - previous_upper_bound)),
            );
        }
        previous_upper_bound = bucket.upper_bound();
        previous_count = bucket.cumulative_count();
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
        let result = get_percentile(99, 5000, &buckets);
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
        let result = get_percentile(50, 5000, &buckets);
        assert_eq!(result, 7.5);
    }

    #[test]
    fn can_observe_request_consumption_and_clear_consumption_metrics() {
        let instance_data = EdgeInstanceData::new("test", &Ulid::new());

        instance_data.observe_request_consumption();
        instance_data.observe_request_consumption();
        instance_data.observe_request_consumption();
        instance_data.observe_request_consumption();

        let serialized = serde_json::to_value(&instance_data).unwrap();
        assert_eq!(
            serialized["requestConsumptionSinceLastReport"],
            serde_json::json!([
                {
                    "meteredGroup": "default",
                    "requests": 4
                }
            ])
        );

        instance_data.clear_time_windowed_metrics();

        let serialized_cleared = serde_json::to_value(&instance_data).unwrap();
        assert_eq!(
            serialized_cleared["requestConsumptionSinceLastReport"],
            serde_json::json!([
                {
                    "meteredGroup": "default",
                    "requests": 0
                }
            ])
        );
    }

    #[test]
    fn can_observe_connection_consumption_with_data_points() {
        let instance_data = EdgeInstanceData::new("test", &Ulid::new());

        instance_data.observe_connection_consumption("/api/client/features", Some(0));
        instance_data.observe_connection_consumption("/api/client/features", Some(0));
        instance_data.observe_connection_consumption("/api/client/features", Some(15001));

        instance_data.observe_connection_consumption("/api/client/metrics", Some(0));
        instance_data.observe_connection_consumption("/api/client/metrics", Some(0));
        instance_data.observe_connection_consumption("/api/client/metrics", Some(60001));

        let serialized = serde_json::to_value(&instance_data).unwrap();
        let connection_data = &serialized["connectionConsumptionSinceLastReport"];

        let actual_features = connection_data["features"][0].clone();
        let actual_metrics = connection_data["metrics"][0].clone();

        let features_data_points = actual_features["dataPoints"].as_array().unwrap();
        assert_eq!(features_data_points.len(), 2);
        assert!(features_data_points.iter().any(|data_point| {
            data_point["interval"] == serde_json::json!([0, 15000]) && data_point["requests"] == 2
        }));
        assert!(features_data_points.iter().any(|data_point| {
            data_point["interval"] == serde_json::json!([15000, 20000])
                && data_point["requests"] == 1
        }));

        let metrics_data_points = actual_metrics["dataPoints"].as_array().unwrap();
        assert_eq!(metrics_data_points.len(), 2);
        assert!(metrics_data_points.iter().any(|data_point| {
            data_point["interval"] == serde_json::json!([0, 60000]) && data_point["requests"] == 2
        }));
        assert!(metrics_data_points.iter().any(|data_point| {
            data_point["interval"] == serde_json::json!([60000, 120000])
                && data_point["requests"] == 1
        }));
    }

    #[test]
    fn test_bucket_boundaries() {
        assert_eq!(
            EdgeInstanceData::get_interval_bucket("/api/client/features", None),
            0..15000
        );
        assert_eq!(
            EdgeInstanceData::get_interval_bucket("/api/client/features", Some(0)),
            0..15000
        );
        assert_eq!(
            EdgeInstanceData::get_interval_bucket("/api/client/features", Some(14999)),
            0..15000
        );
        assert_eq!(
            EdgeInstanceData::get_interval_bucket("/api/client/features", Some(15000)),
            0..15000
        );
        assert_eq!(
            EdgeInstanceData::get_interval_bucket("/api/client/features", Some(15001)),
            15000..20000
        );
        assert_eq!(
            EdgeInstanceData::get_interval_bucket("/api/client/features", Some(19999)),
            15000..20000
        );
        assert_eq!(
            EdgeInstanceData::get_interval_bucket("/api/client/features", Some(20000)),
            20000..25000
        );

        assert_eq!(
            EdgeInstanceData::get_interval_bucket("/api/client/metrics", None),
            0..60000
        );
        assert_eq!(
            EdgeInstanceData::get_interval_bucket("/api/client/metrics", Some(0)),
            0..60000
        );
        assert_eq!(
            EdgeInstanceData::get_interval_bucket("/api/client/metrics", Some(59999)),
            0..60000
        );
        assert_eq!(
            EdgeInstanceData::get_interval_bucket("/api/client/metrics", Some(60000)),
            0..60000
        );
        assert_eq!(
            EdgeInstanceData::get_interval_bucket("/api/client/metrics", Some(60001)),
            60000..120000
        );
        assert_eq!(
            EdgeInstanceData::get_interval_bucket("/api/client/metrics", Some(119999)),
            60000..120000
        );
        assert_eq!(
            EdgeInstanceData::get_interval_bucket("/api/client/metrics", Some(120000)),
            120000..180000
        );

        // Test intervals greater than 1 hour (3600000 ms)
        assert_eq!(
            EdgeInstanceData::get_interval_bucket("/api/client/features", Some(3600001)),
            3600000..3600000
        );
        assert_eq!(
            EdgeInstanceData::get_interval_bucket("/api/client/metrics", Some(3600001)),
            3600000..3600000
        );
        assert_eq!(
            EdgeInstanceData::get_interval_bucket("/api/client/features", Some(7200000)),
            3600000..3600000
        );
        assert_eq!(
            EdgeInstanceData::get_interval_bucket("/api/client/metrics", Some(7200000)),
            3600000..3600000
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
