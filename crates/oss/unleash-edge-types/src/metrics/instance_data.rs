use crate::BuildInfo;
use crate::metrics::{
    BUCKET_SIZE_FEATURES, BUCKET_SIZE_METRICS, ConnectionConsumptionData, ConnectionMetricsType,
    DEFAULT_FEATURES_INTERVAL, DEFAULT_METRICS_INTERVAL, DESIRED_URLS, DataPoint, ENDPOINT_LABEL,
    InstanceTraffic, LatencyMetrics, MAX_BUCKET_INTERVAL, METHOD_LABEL, ProcessMetrics,
    RequestConsumptionData, RequestStats, STATUS_LABEL, UpstreamLatency,
};
use ahash::{HashMap, HashSet};
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use prometheus::gather;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::sync::atomic::{AtomicU64, Ordering};
use ulid::Ulid;

pub const CONNECTED_STREAMING_CLIENTS: &str = "connected_streaming_clients";

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct ApiKeyIdentity {
    pub environment: String,
    pub projects: Vec<String>,
}

impl From<&EdgeApiKeyRevisionId> for ApiKeyIdentity {
    fn from(value: &EdgeApiKeyRevisionId) -> Self {
        Self {
            environment: value.environment.clone(),
            projects: value.projects.clone(),
        }
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EdgeApiKeyRevisionId {
    pub environment: String,
    pub projects: Vec<String>,
    pub revision_id: usize,
    pub last_updated: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnricherMetrics {
    pub traffic: LatencyMetrics,
    pub errors: u64,
    pub timeouts: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EdgeInstanceData {
    pub identifier: String,
    pub app_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hosting: Option<Hosting>,
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
    #[serde(
        default,
        serialize_with = "serialize_map_to_values",
        deserialize_with = "deserialize_array_to_dashmap"
    )]
    pub edge_api_key_revision_ids: DashMap<ApiKeyIdentity, EdgeApiKeyRevisionId>,
    #[serde(default)]
    pub enricher_metrics: Option<EnricherMetrics>,
}

fn deserialize_array_to_dashmap<'de, D>(
    de: D,
) -> Result<DashMap<ApiKeyIdentity, EdgeApiKeyRevisionId>, D::Error>
where
    D: Deserializer<'de>,
{
    let vec = Vec::<EdgeApiKeyRevisionId>::deserialize(de)?;
    Ok(vec
        .into_iter()
        .map(|item| (ApiKeyIdentity::from(&item), item))
        .collect())
}
fn serialize_map_to_values<S>(
    value: &DashMap<ApiKeyIdentity, EdgeApiKeyRevisionId>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let values: Vec<EdgeApiKeyRevisionId> = value.iter().map(|item| item.value().clone()).collect();
    values.serialize(serializer)
}

#[derive(Debug, Copy, Clone, Deserialize, Serialize, Eq, PartialEq)]
pub enum Hosting {
    #[serde(rename = "self-hosted")]
    SelfHosted,
    #[serde(rename = "hosted")]
    Hosted,
    #[serde(rename = "enterprise-self-hosted")]
    EnterpriseSelfHosted,
}

/// Most distinct instances we hold reports from, nested ones included. Folding stops the buffer
/// growing with time, but a restarted instance comes back under a new identifier and nothing
/// evicts the old one, so without a cap a long upstream outage still grows it.
pub const MAX_DISTINCT_INSTANCES: usize = 250;

/// How many reports there are and how many distinct instances they came from, nested ones
/// included.
pub fn report_counts<'a>(
    reports: impl IntoIterator<Item = &'a EdgeInstanceData>,
) -> (usize, usize) {
    let mut pending: Vec<&EdgeInstanceData> = reports.into_iter().collect();
    let mut total = 0;
    let mut distinct: HashSet<&str> = HashSet::default();

    while let Some(report) = pending.pop() {
        total += 1;
        distinct.insert(report.identifier.as_str());
        pending.extend(report.connected_edges.iter());
    }

    (total, distinct.len())
}

/// Fold a report into a buffer holding at most one entry per instance.
///
/// Reports from the same instance are not interchangeable. The `*_since_last_report` fields are
/// deltas covering the window since that instance last posted, and `connected_edges` holds
/// whatever reached it during that same window - so keeping only the newest report would throw
/// away a window of consumption and a whole set of downstream reports.
pub fn merge_report(into: &mut Vec<EdgeInstanceData>, mut report: EdgeInstanceData) {
    match into
        .iter_mut()
        .find(|held| held.identifier == report.identifier)
    {
        Some(held) => held.absorb(report),
        None => {
            // An instance running a version without this fold can send us duplicates of its
            // own, so flatten the report's subtree before we take it on.
            let connected_edges = std::mem::take(&mut report.connected_edges);
            merge_reports(&mut report.connected_edges, connected_edges);
            into.push(report);
        }
    }
}

/// [`merge_report`] over a batch.
pub fn merge_reports(into: &mut Vec<EdgeInstanceData>, incoming: Vec<EdgeInstanceData>) {
    for report in incoming {
        merge_report(into, report);
    }
}

impl EdgeInstanceData {
    pub fn new(app_name: &str, identifier: &Ulid, hosting: Option<Hosting>) -> Self {
        let build_info = BuildInfo::default();
        Self {
            identifier: identifier.to_string(),
            hosting,
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
            edge_api_key_revision_ids: DashMap::default(),
            enricher_metrics: None,
        }
    }

    pub fn clear_time_windowed_metrics(&self) {
        self.requests_since_last_report.clear();
        self.connection_consumption_since_last_report.reset();
        self.request_consumption_since_last_report.reset();
    }

    /// Absorb a later report from this same instance.
    ///
    /// Destructured on purpose: add a field to this struct and this stops compiling until
    /// someone decides whether it accumulates or is superseded.
    fn absorb(&mut self, newer: Self) {
        let Self {
            identifier: _,
            app_name,
            hosting,
            region,
            edge_version,
            process_metrics,
            started,
            traffic,
            latency_upstream,
            requests_since_last_report,
            connected_streaming_clients,
            connected_edges,
            connection_consumption_since_last_report,
            request_consumption_since_last_report,
            edge_api_key_revision_ids,
            enricher_metrics,
        } = newer;

        // Superseded: point in time, or counted since that process started, so the newer
        // value already covers what the older report said.
        self.app_name = app_name;
        self.hosting = hosting;
        self.region = region;
        self.edge_version = edge_version;
        self.process_metrics = process_metrics;
        self.started = started;
        self.traffic = traffic;
        self.latency_upstream = latency_upstream;
        self.connected_streaming_clients = connected_streaming_clients;
        self.enricher_metrics = enricher_metrics;
        self.edge_api_key_revision_ids = edge_api_key_revision_ids;

        // Accumulated. Each report covers a different window, so the windows add up.
        for (endpoint, stats) in requests_since_last_report {
            let RequestStats {
                requests_200,
                requests_304,
            } = stats;
            let held = self.requests_since_last_report.entry(endpoint).or_default();
            held.requests_200
                .fetch_add(requests_200.into_inner(), Ordering::Relaxed);
            held.requests_304
                .fetch_add(requests_304.into_inner(), Ordering::Relaxed);
        }
        self.connection_consumption_since_last_report
            .absorb(connection_consumption_since_last_report);
        self.request_consumption_since_last_report
            .absorb(request_consumption_since_last_report);

        // The same downstream instance can appear in both windows, with different deltas.
        merge_reports(&mut self.connected_edges, connected_edges);
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

    pub fn observe_api_key_refresh(
        &self,
        environment: String,
        projects: Vec<String>,
        revision_id: usize,
        last_updated: DateTime<Utc>,
    ) {
        let revision_info = EdgeApiKeyRevisionId {
            environment,
            projects,
            revision_id,
            last_updated,
        };
        self.edge_api_key_revision_ids
            .entry(ApiKeyIdentity::from(&revision_info))
            .and_modify(|entry| {
                entry.revision_id = revision_id;
                entry.last_updated = last_updated;
            })
            .or_insert(revision_info);
    }

    pub fn observe(&self, connected_instances: Vec<EdgeInstanceData>, base_path: &str) -> Self {
        let mut observed = self.clone();
        let mut cpu_seconds = 0;
        let mut resident_memory = 0;
        let mut get_requests = HashMap::default();
        let mut post_requests = HashMap::default();
        let mut access_denied = HashMap::default();
        let mut no_change = HashMap::default();
        let mut enricher_errors = 0;
        let mut enricher_timeouts = 0;

        for family in gather().iter() {
            match family.name() {
                crate::metrics::HTTP_REQUESTS_DURATION => {
                    family
                        .get_metric()
                        .iter()
                        .filter(|m| {
                            m.get_label().iter().any(|l| {
                                l.name() == ENDPOINT_LABEL
                                    && DESIRED_URLS
                                        .iter()
                                        .any(|desired| l.value().ends_with(desired))
                            }) && m.get_label().iter().any(|l| {
                                l.name() == STATUS_LABEL
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
                                .find(|l| l.name() == ENDPOINT_LABEL)
                                .unwrap()
                                .value()
                                .strip_prefix(base_path)
                                .unwrap();
                            let method = labels
                                .iter()
                                .find(|l| l.name() == METHOD_LABEL)
                                .unwrap()
                                .value();
                            let status = labels
                                .iter()
                                .find(|l| l.name() == STATUS_LABEL)
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
                            let total = m.get_histogram().get_sample_sum(); // already in ms
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
                CONNECTED_STREAMING_CLIENTS => {
                    if let Some(connected_streaming_clients) = family.get_metric().last() {
                        observed.connected_streaming_clients =
                            connected_streaming_clients.get_gauge().value() as u64;
                    }
                }
                "context_enricher_duration_milliseconds" => {
                    if let Some(metric) = family.get_metric().last() {
                        let histogram = metric.get_histogram();
                        let count = histogram.get_sample_count();
                        observed.enricher_metrics = Some(EnricherMetrics {
                            traffic: LatencyMetrics {
                                avg: if count == 0 {
                                    0.0
                                } else {
                                    round_to_3_decimals(histogram.get_sample_sum() / count as f64)
                                },
                                count: count as f64,
                                p99: if count == 0 {
                                    0.0
                                } else {
                                    get_percentile(99, count, histogram.get_bucket())
                                },
                            },
                            errors: 0,
                            timeouts: 0,
                        });
                    }
                }
                "context_enricher_errors_total" => {
                    if let Some(metric) = family.get_metric().last() {
                        enricher_errors = metric.get_counter().value() as u64;
                    }
                }
                "context_enricher_timeouts_total" => {
                    if let Some(metric) = family.get_metric().last() {
                        enricher_timeouts = metric.get_counter().value() as u64;
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
        if let Some(enricher_metrics) = &mut observed.enricher_metrics {
            enricher_metrics.errors = enricher_errors;
            enricher_metrics.timeouts = enricher_timeouts;
        }
        merge_reports(&mut observed.connected_edges, connected_instances);
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
    use prometheus::{register_histogram, register_int_counter};
    use serde_json::json;

    #[test]
    fn two_reports_from_one_instance_add_their_consumption() {
        let id = Ulid::generate();
        let first = EdgeInstanceData::new("downstream", &id, None);
        first.observe_request_consumption();
        first.observe_request_consumption();
        let second = EdgeInstanceData::new("downstream", &id, None);
        second.observe_request_consumption();

        let mut buffer = vec![first];
        merge_reports(&mut buffer, vec![second]);

        assert_eq!(buffer.len(), 1, "one instance should hold one entry");
        assert_eq!(
            buffer[0]
                .request_consumption_since_last_report
                .get_requests("default"),
            3,
            "each report covers its own window, so the windows add up"
        );
    }

    #[test]
    fn a_grandchild_seen_in_two_windows_has_its_deltas_added() {
        let parent_id = Ulid::generate();
        let child_id = Ulid::generate();

        let mut first = EdgeInstanceData::new("parent", &parent_id, None);
        let child_first = EdgeInstanceData::new("child", &child_id, None);
        child_first.observe_request_consumption();
        child_first.observe_request_consumption();
        first.connected_edges = vec![child_first];

        let mut second = EdgeInstanceData::new("parent", &parent_id, None);
        let child_second = EdgeInstanceData::new("child", &child_id, None);
        child_second.observe_request_consumption();
        second.connected_edges = vec![
            child_second,
            EdgeInstanceData::new("other-child", &Ulid::generate(), None),
        ];

        first.absorb(second);

        assert_eq!(first.connected_edges.len(), 2);
        let child = first
            .connected_edges
            .iter()
            .find(|held| held.identifier == child_id.to_string())
            .expect("the child seen in both windows should survive");
        assert_eq!(
            child
                .request_consumption_since_last_report
                .get_requests("default"),
            3,
            "each report carries only the window it saw, so a child in both windows adds up"
        );
    }

    #[test]
    fn two_reports_add_their_per_endpoint_request_counts() {
        let id = Ulid::generate();
        let first = EdgeInstanceData::new("downstream", &id, None);
        first.observe_request("/api/client/features", 200);
        first.observe_request("/api/client/features", 200);
        let second = EdgeInstanceData::new("downstream", &id, None);
        second.observe_request("/api/client/features", 200);
        second.observe_request("/api/client/features", 304);

        let mut buffer = vec![first];
        merge_reports(&mut buffer, vec![second]);

        let stats = buffer[0]
            .requests_since_last_report
            .get("/api/client/features")
            .expect("the endpoint we observed should be there");
        assert_eq!(stats.requests_200.load(Ordering::Relaxed), 3);
        assert_eq!(stats.requests_304.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn two_reports_add_their_connection_consumption() {
        let id = Ulid::generate();
        let first = EdgeInstanceData::new("downstream", &id, None);
        first.observe_connection_consumption("/api/client/features", Some(15000));
        first.observe_connection_consumption("/api/client/features", Some(15000));
        let second = EdgeInstanceData::new("downstream", &id, None);
        second.observe_connection_consumption("/api/client/features", Some(15000));

        let mut buffer = vec![first];
        merge_reports(&mut buffer, vec![second]);

        let serialized = serde_json::to_value(&buffer[0]).expect("should serialize");
        assert_eq!(
            serialized["connectionConsumptionSinceLastReport"]["features"][0]["dataPoints"][0]["requests"],
            json!(3),
            "the same interval bucket in two windows is one bucket counted twice over"
        );
    }

    #[test]
    fn cumulative_fields_are_superseded_rather_than_summed() {
        let id = Ulid::generate();
        let mut first = EdgeInstanceData::new("downstream", &id, None);
        first.connected_streaming_clients = 5;
        let mut second = EdgeInstanceData::new("downstream", &id, None);
        second.connected_streaming_clients = 3;

        first.absorb(second);

        assert_eq!(
            first.connected_streaming_clients, 3,
            "a gauge reports a level, not a delta, so the newer reading wins"
        );
    }

    #[test]
    fn reports_from_different_instances_stay_separate() {
        let mut buffer = vec![EdgeInstanceData::new("one", &Ulid::generate(), None)];
        merge_reports(
            &mut buffer,
            vec![EdgeInstanceData::new("two", &Ulid::generate(), None)],
        );
        assert_eq!(buffer.len(), 2);
    }

    #[test]
    fn observes_context_enricher_metrics() {
        let duration = register_histogram!(
            "context_enricher_duration_milliseconds",
            "Context enrichment duration",
            vec![1.0, 5.0, 10.0]
        )
        .unwrap();
        let errors =
            register_int_counter!("context_enricher_errors_total", "Enrichment errors").unwrap();
        let timeouts =
            register_int_counter!("context_enricher_timeouts_total", "Enrichment timeouts")
                .unwrap();

        duration.observe(2.0);
        duration.observe(4.0);
        errors.inc();
        timeouts.inc();

        let data = EdgeInstanceData::new("test", &Ulid::generate(), None).observe(vec![], "");
        let enricher = serde_json::to_value(data.enricher_metrics).unwrap();
        assert_eq!(enricher["traffic"]["count"], 2.0);
        assert_eq!(enricher["traffic"]["avg"], 3.0);
        assert_eq!(enricher["errors"], 1);
        assert_eq!(enricher["timeouts"], 1);
    }

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
        let instance_data = EdgeInstanceData::new("test", &Ulid::generate(), None);

        instance_data.observe_request_consumption();
        instance_data.observe_request_consumption();
        instance_data.observe_request_consumption();
        instance_data.observe_request_consumption();

        let serialized = serde_json::to_value(&instance_data).unwrap();
        assert_eq!(
            serialized["requestConsumptionSinceLastReport"],
            json!([
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
            json!([
                {
                    "meteredGroup": "default",
                    "requests": 0
                }
            ])
        );
    }

    #[test]
    fn can_observe_connection_consumption_with_data_points() {
        let instance_data = EdgeInstanceData::new("test", &Ulid::generate(), None);

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
            data_point["interval"] == json!([0, 15000]) && data_point["requests"] == 2
        }));
        assert!(features_data_points.iter().any(|data_point| {
            data_point["interval"] == json!([15000, 20000]) && data_point["requests"] == 1
        }));

        let metrics_data_points = actual_metrics["dataPoints"].as_array().unwrap();
        assert_eq!(metrics_data_points.len(), 2);
        assert!(metrics_data_points.iter().any(|data_point| {
            data_point["interval"] == json!([0, 60000]) && data_point["requests"] == 2
        }));
        assert!(metrics_data_points.iter().any(|data_point| {
            data_point["interval"] == json!([60000, 120000]) && data_point["requests"] == 1
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

    #[test]
    fn serializes_hosting_if_and_only_if_present() {
        let self_hosted =
            EdgeInstanceData::new("test", &Ulid::generate(), Some(Hosting::SelfHosted));
        let hosted = EdgeInstanceData::new("test", &Ulid::generate(), Some(Hosting::Hosted));
        let no_data = EdgeInstanceData::new("test", &Ulid::generate(), None);

        let serialized_self_hosted = serde_json::to_value(&self_hosted).unwrap();
        assert_eq!(serialized_self_hosted["hosting"], "self-hosted");

        let serialized_hosted = serde_json::to_value(&hosted).unwrap();
        assert_eq!(serialized_hosted["hosting"], "hosted");

        let serialized_no_data = serde_json::to_value(&no_data).unwrap();

        if let Some(map) = serialized_no_data.as_object() {
            assert!(!map.contains_key("hosting"));
        } else {
            panic!("Expected JSON value to be an object");
        }
    }

    #[test]
    fn can_observe_refreshed_api_key() {
        let self_hosted = EdgeInstanceData::new("test", &Ulid::generate(), Some(Hosting::Hosted));
        assert!(self_hosted.edge_api_key_revision_ids.is_empty());
        let now = Utc::now();
        self_hosted.observe_api_key_refresh(
            "development".to_string(),
            vec!["*".to_string()],
            500,
            now,
        );
        assert_eq!(self_hosted.edge_api_key_revision_ids.len(), 1);
    }

    #[test]
    fn updating_same_key_updates_revision_id_and_timestamp() {
        let self_hosted = EdgeInstanceData::new("test", &Ulid::generate(), Some(Hosting::Hosted));
        assert!(self_hosted.edge_api_key_revision_ids.is_empty());
        let now = Utc::now();
        let key = ApiKeyIdentity {
            environment: "development".to_string(),
            projects: vec!["*".to_string()],
        };
        self_hosted.observe_api_key_refresh(
            "development".to_string(),
            vec!["*".to_string()],
            500,
            now,
        );
        assert_eq!(self_hosted.edge_api_key_revision_ids.len(), 1);
        let observed = self_hosted
            .edge_api_key_revision_ids
            .get(&key)
            .map(|k| k.value().clone())
            .unwrap();
        assert_eq!(observed.revision_id, 500);
        assert_eq!(observed.last_updated, now);
        let new_update = Utc::now();
        self_hosted.observe_api_key_refresh(
            "development".to_string(),
            vec!["*".to_string()],
            505,
            new_update,
        );
        let observed = self_hosted
            .edge_api_key_revision_ids
            .get(&key)
            .map(|k| k.value().clone())
            .unwrap();
        assert_eq!(observed.revision_id, 505);
        assert_eq!(observed.last_updated, new_update);
    }

    #[test]
    pub fn unique_keys_all_get_stored() {
        let self_hosted = EdgeInstanceData::new("test", &Ulid::generate(), Some(Hosting::Hosted));
        assert!(self_hosted.edge_api_key_revision_ids.is_empty());
        let now = Utc::now();
        let dev_key = ApiKeyIdentity {
            environment: "development".to_string(),
            projects: vec!["*".to_string()],
        };
        self_hosted.observe_api_key_refresh(
            "development".to_string(),
            vec!["*".to_string()],
            500,
            now,
        );
        assert_eq!(self_hosted.edge_api_key_revision_ids.len(), 1);
        let observed = self_hosted
            .edge_api_key_revision_ids
            .get(&dev_key)
            .map(|k| k.value().clone())
            .unwrap();
        assert_eq!(observed.revision_id, 500);
        assert_eq!(observed.last_updated, now);
        let new_update = Utc::now();
        self_hosted.observe_api_key_refresh(
            "production".to_string(),
            vec!["*".to_string()],
            505,
            new_update,
        );
        let prod_key = ApiKeyIdentity {
            environment: "production".to_string(),
            projects: vec!["*".to_string()],
        };
        let observed = self_hosted
            .edge_api_key_revision_ids
            .get(&prod_key)
            .map(|k| k.value().clone())
            .unwrap();
        assert_eq!(observed.revision_id, 505);
        assert_eq!(observed.last_updated, new_update);
        assert_eq!(self_hosted.edge_api_key_revision_ids.len(), 2);
    }

    #[test]
    pub fn successfully_deserializes_with_no_edge_api_key_revision_id_entry() {
        let requests_since_last_report: DashMap<String, RequestStats> = DashMap::default();

        let json = json!({
            "identifier": Ulid::generate().to_string(),
            "appName": "test",
            "hosting": "hosted",
            "edgeVersion": "1.0.0",
            "region": "us-west-1",
            "started": Utc::now(),
            "traffic": InstanceTraffic::default(),
            "latencyUpstream": UpstreamLatency::default(),
            "requestsSinceLastReport": requests_since_last_report,
            "connectedStreamingClients": 0,
            "connectedEdges": Vec::<String>::new(),
            "connectionConsumptionSinceLastReport": ConnectionConsumptionData::default(),
            "requestConsumptionSinceLastReport": RequestConsumptionData::default(),
        });
        let instance_data = serde_json::from_value::<EdgeInstanceData>(json);
        match instance_data {
            Ok(data) => assert_eq!(data.edge_api_key_revision_ids.len(), 0),
            Err(e) => panic!("Failed to deserialize instance data: {}", e),
        }
    }

    #[test]
    pub fn deserializes_list_of_edge_api_key_revision_ids_to_map_internally() {
        let requests_since_last_report: DashMap<String, RequestStats> = DashMap::default();
        let json = json!({
                        "identifier": Ulid::generate().to_string(),
                        "appName": "test",
                        "hosting": "hosted",
                        "edgeVersion": "1.0.0",
                        "region": "us-west-1",
                        "started": Utc::now(),
                        "traffic": InstanceTraffic::default(),
                        "latencyUpstream": UpstreamLatency::default(),
                        "requestsSinceLastReport": requests_since_last_report,
                        "connectedStreamingClients": 0,
                        "connectedEdges": Vec::<String>::new(),
                        "connectionConsumptionSinceLastReport": ConnectionConsumptionData::default(),
                        "requestConsumptionSinceLastReport": RequestConsumptionData::default(),
                        "edgeApiKeyRevisionIds": vec![
            EdgeApiKeyRevisionId {
                environment: "development".to_string(),
                projects: vec!["*".to_string()],
                revision_id: 100,
                last_updated: Utc::now(),
            }, EdgeApiKeyRevisionId {
                environment: "production".to_string(),
                projects: vec!["*".to_string()],
                revision_id: 102,
                last_updated: Utc::now(),
            }
        ]});
        let instance_data = serde_json::from_value::<EdgeInstanceData>(json);
        match instance_data {
            Ok(data) => assert_eq!(data.edge_api_key_revision_ids.len(), 2),
            Err(e) => panic!("Failed to deserialize instance data: {}", e),
        }
    }

    mod folding {
        use super::*;
        use proptest::prelude::*;

        fn report(
            identifier: u8,
            requests: u64,
            connected_edges: Vec<EdgeInstanceData>,
        ) -> EdgeInstanceData {
            let mut report = EdgeInstanceData::new("edge", &Ulid::generate(), None);
            report.identifier = identifier.to_string();
            for _ in 0..requests {
                report.observe_request_consumption();
            }
            report.connected_edges = connected_edges;
            report
        }

        // Identifiers come from a small pool so one instance turns up in several reports, at
        // any depth, the way it does when an upstream has been failing for a while.
        fn reports() -> impl Strategy<Value = Vec<EdgeInstanceData>> {
            let leaf = (0..4u8, 0..5u64).prop_map(|(id, requests)| report(id, requests, vec![]));
            let tree = leaf.prop_recursive(3, 32, 4, |inner| {
                (0..4u8, 0..5u64, prop::collection::vec(inner, 0..4))
                    .prop_map(|(id, requests, children)| report(id, requests, children))
            });
            prop::collection::vec(tree, 0..6)
        }

        fn requests(reports: &[EdgeInstanceData]) -> u64 {
            reports
                .iter()
                .map(|report| {
                    report
                        .request_consumption_since_last_report
                        .get_requests("default")
                        + requests(&report.connected_edges)
                })
                .sum()
        }

        fn one_entry_per_instance(reports: &[EdgeInstanceData]) -> bool {
            let identifiers: HashSet<&str> = reports
                .iter()
                .map(|report| report.identifier.as_str())
                .collect();
            identifiers.len() == reports.len()
                && reports
                    .iter()
                    .all(|report| one_entry_per_instance(&report.connected_edges))
        }

        proptest! {
            #[test]
            fn folding_neither_gains_nor_loses_consumption(reports in reports()) {
                let consumption = requests(&reports);
                let (_, distinct) = report_counts(&reports);

                let mut buffer = vec![];
                merge_reports(&mut buffer, reports);

                prop_assert_eq!(requests(&buffer), consumption);
                prop_assert_eq!(report_counts(&buffer).1, distinct);
                prop_assert!(one_entry_per_instance(&buffer));
            }
        }
    }
}
