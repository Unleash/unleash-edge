use crate::MetricsKey;
use ahash::HashMap;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use unleash_types::client_metrics::{ClientApplication, ClientMetricsEnv, ConnectVia, ImpactMetricEnv};
use utoipa::ToSchema;

pub mod batching;
pub mod instance_data;

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsumptionGroup {
    pub metered_group: String,
    pub data_points: Vec<DataPoint>,
}

#[derive(Debug, Default, Clone, Copy, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LatencyMetrics {
    pub avg: f64,
    pub count: f64,
    pub p99: f64,
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

pub(crate) const HTTP_REQUESTS_TOTAL: &str = "http_requests_total";
pub(crate) const HTTP_REQUESTS_DURATION: &str = "http_server_duration_milliseconds";
pub(crate) const HTTP_RESPONSE_SIZE: &str = "http_response_size";
pub const DESIRED_URLS: [&str; 6] = [
    "/api/client/features",
    "/api/client/metrics",
    "/api/client/metrics/bulk",
    "/api/client/metrics/edge",
    "/api/frontend",
    "/api/proxy",
];

#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub struct ApplicationKey {
    pub app_name: String,
    pub instance_id: String,
}

impl From<ClientApplication> for ApplicationKey {
    fn from(value: ClientApplication) -> Self {
        Self {
            app_name: value.app_name,
            instance_id: value.instance_id.unwrap_or_else(|| "default".into()),
        }
    }
}

impl From<ClientMetricsEnv> for MetricsKey {
    fn from(value: ClientMetricsEnv) -> Self {
        Self {
            app_name: value.app_name,
            feature_name: value.feature_name,
            timestamp: value.timestamp,
            environment: value.environment,
        }
    }
}

#[derive(Debug, Clone, Eq, Deserialize, Serialize, ToSchema, Hash, PartialEq)]
pub struct ImpactMetricsKey {
    pub app_name: String,
    pub environment: String,
}

impl From<&ImpactMetricEnv> for ImpactMetricsKey {
    fn from(value: &ImpactMetricEnv) -> Self {
        Self {
            app_name: value.app_name.clone(),
            environment: value.environment.clone(),
        }
    }
}

#[derive(Default, Debug)]
pub struct MetricsCache {
    pub applications: DashMap<ApplicationKey, ClientApplication>,
    pub metrics: DashMap<MetricsKey, ClientMetricsEnv>,
    pub impact_metrics: DashMap<ImpactMetricsKey, Vec<ImpactMetricEnv>>,
}