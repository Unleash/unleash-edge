use crate::types::{BatchMetricsRequestBody, EdgeToken};
use actix_web::web::Data;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use itertools::Itertools;
use lazy_static::lazy_static;
use prometheus::{Histogram, IntCounterVec, register_histogram, register_int_counter_vec};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
};
use tracing::{debug, instrument};
use unleash_types::client_metrics::SdkType::Backend;
use unleash_types::client_metrics::{ClientApplication, ClientMetrics, ClientMetricsEnv, ConnectVia, ImpactMetric, ImpactMetricEnv, MetricSample, MetricsMetadata};
use utoipa::ToSchema;

pub const UPSTREAM_MAX_BODY_SIZE: usize = 100 * 1024;
pub const BATCH_BODY_SIZE: usize = 95 * 1024;

lazy_static! {
    pub static ref METRICS_SIZE_HISTOGRAM: Histogram = register_histogram!(
        "metrics_size_in_bytes",
        "Size of metrics when posting",
        vec![
            1000.0, 10000.0, 20000.0, 50000.0, 75000.0, 100000.0, 250000.0, 500000.0, 1000000.0
        ]
    )
    .unwrap();
    pub static ref FEATURE_TOGGLE_USAGE_TOTAL: IntCounterVec = register_int_counter_vec!(
        "feature_toggle_usage_total",
        "Number of times a feature flag has been used",
        &["appName", "toggle", "active"]
    )
    .unwrap();
}

#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub(crate) struct ApplicationKey {
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

impl From<ImpactMetricEnv> for ImpactMetricsKey {
    fn from(value: ImpactMetricEnv) -> Self {
        Self {
            app_name: value.app_name,
            environment: value.environment,
        }
    }
}

#[derive(Debug, Clone, Eq, Deserialize, Serialize, ToSchema)]
pub struct MetricsKey {
    pub app_name: String,
    pub feature_name: String,
    pub environment: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Eq, Deserialize, Serialize, ToSchema)]
pub struct ImpactMetricsKey {
    pub app_name: String,
    pub environment: String,
}

impl Hash for ImpactMetricsKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.app_name.hash(state);
        self.environment.hash(state);
    }
}

impl PartialEq for ImpactMetricsKey {
    fn eq(&self, other: &Self) -> bool {
        self.app_name == other.app_name
            && self.environment == other.environment
    }
}

impl Hash for MetricsKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.app_name.hash(state);
        self.feature_name.hash(state);
        self.environment.hash(state);
        to_time_key(&self.timestamp).hash(state);
    }
}

fn to_time_key(timestamp: &DateTime<Utc>) -> String {
    format!("{}", timestamp.format("%Y-%m-%d %H"))
}

impl PartialEq for MetricsKey {
    fn eq(&self, other: &Self) -> bool {
        let other_hour_bin = to_time_key(&other.timestamp);
        let self_hour_bin = to_time_key(&self.timestamp);

        self.app_name == other.app_name
            && self.feature_name == other.feature_name
            && self.environment == other.environment
            && self_hour_bin == other_hour_bin
    }
}

fn convert_to_impact_metrics_env(metrics: Vec<ImpactMetric>, app_name: String, environment: String) -> Vec<ImpactMetricEnv> {
    metrics.into_iter()
        .map(|metric| ImpactMetricEnv::new(metric, app_name.clone(), environment.clone()))
        .collect()
}

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct MetricsBatch {
    pub applications: Vec<ClientApplication>,
    pub metrics: Vec<ClientMetricsEnv>,
    #[serde(default, skip_serializing_if = "Vec::is_empty", rename = "impactMetrics")]
    pub impact_metrics: Vec<ImpactMetricEnv>,
}

#[derive(Default, Debug)]
pub struct MetricsCache {
    pub(crate) applications: DashMap<ApplicationKey, ClientApplication>,
    pub(crate) metrics: DashMap<MetricsKey, ClientMetricsEnv>,
    pub(crate) impact_metrics: DashMap<ImpactMetricsKey, Vec<ImpactMetricEnv>>,
}

pub(crate) fn size_of_batch(batch: &MetricsBatch) -> usize {
    serde_json::to_string(batch).map(|s| s.len()).unwrap_or(0)
}

pub(crate) fn register_client_application(
    edge_token: EdgeToken,
    connect_via: &ConnectVia,
    client_application: ClientApplication,
    metrics_cache: Data<MetricsCache>,
) {
    let updated_with_connection_info = client_application.connect_via(
        connect_via.app_name.as_str(),
        connect_via.instance_id.as_str(),
    );
    let to_write = ClientApplication {
        environment: edge_token.environment,
        projects: Some(edge_token.projects),
        metadata: MetricsMetadata {
            sdk_type: Some(Backend),
            ..updated_with_connection_info.metadata
        },
        ..updated_with_connection_info
    };
    metrics_cache.applications.insert(
        ApplicationKey {
            app_name: to_write.app_name.clone(),
            instance_id: to_write
                .instance_id
                .clone()
                .unwrap_or_else(|| ulid::Ulid::new().to_string()),
        },
        to_write,
    );
}

pub(crate) fn register_client_metrics(
    edge_token: EdgeToken,
    metrics: ClientMetrics,
    metrics_cache: Data<MetricsCache>,
) {
    let environment = edge_token
        .environment
        .clone()
        .unwrap_or_else(|| "development".into());

    let client_metrics_env = unleash_types::client_metrics::from_bucket_app_name_and_env(
        metrics.bucket,
        metrics.app_name.clone(),
        environment.clone(),
        metrics.metadata.clone(),
    );

    if let Some(impact_metrics) = metrics.impact_metrics {
        let impact_metrics_env = convert_to_impact_metrics_env(impact_metrics, metrics.app_name.clone(), environment);
        metrics_cache.sink_impact_metrics(impact_metrics_env);
    }

    metrics_cache.sink_metrics(&client_metrics_env);
}

/***
   Will filter out metrics that do not belong to the environment that edge_token has access to
*/
pub(crate) fn register_bulk_metrics(
    metrics_cache: &MetricsCache,
    connect_via: &ConnectVia,
    edge_token: &EdgeToken,
    metrics: BatchMetricsRequestBody,
) {
    let updated: BatchMetricsRequestBody = BatchMetricsRequestBody {
        applications: metrics.applications.clone(),
        metrics: metrics
            .metrics
            .iter()
            .filter(|m| {
                edge_token
                    .environment
                    .clone()
                    .map(|e| e == m.environment)
                    .unwrap_or(false)
            })
            .cloned()
            .collect(),
        impact_metrics: metrics.impact_metrics.clone(),
    };
    metrics_cache.sink_bulk_metrics(updated, connect_via);
}

pub(crate) fn sendable(batch: &MetricsBatch) -> bool {
    size_of_batch(batch) < UPSTREAM_MAX_BODY_SIZE
}

#[instrument(skip(batch))]
pub(crate) fn cut_into_sendable_batches(batch: MetricsBatch) -> Vec<MetricsBatch> {
    let batch_count = (size_of_batch(&batch) / BATCH_BODY_SIZE) + 1;
    let apps_count = batch.applications.len();
    let apps_per_batch = apps_count / batch_count;

    let metrics_count = batch.metrics.len();
    let metrics_per_batch = metrics_count / batch_count;

    let impact_metrics_count = batch.impact_metrics.len();
    let impact_metrics_per_batch = if impact_metrics_count > 0 { impact_metrics_count / batch_count } else { 0 };

    debug!(
        "Batch count: {batch_count}. Apps per batch: {apps_per_batch}, Metrics per batch: {metrics_per_batch}, Impact metrics per batch: {impact_metrics_per_batch}"
    );
    (0..=batch_count)
        .map(|counter| {
            let apps_iter = batch.applications.iter();
            let metrics_iter = batch.metrics.iter();
            let apps_take = if apps_per_batch == 0 && counter == 0 {
                apps_count
            } else {
                apps_per_batch
            };
            let metrics_take = if metrics_per_batch == 0 && counter == 0 {
                metrics_count
            } else {
                metrics_per_batch
            };

            let impact_metrics_iter = batch.impact_metrics.iter();
            let impact_metrics_take = if impact_metrics_per_batch == 0 && counter == 0 {
                impact_metrics_count
            } else {
                impact_metrics_per_batch
            };

            MetricsBatch {
                metrics: metrics_iter
                    .skip(counter * metrics_per_batch)
                    .take(metrics_take)
                    .cloned()
                    .collect(),
                applications: apps_iter
                    .skip(counter * apps_per_batch)
                    .take(apps_take)
                    .cloned()
                    .collect(),
                impact_metrics: impact_metrics_iter
                    .skip(counter * impact_metrics_per_batch)
                    .take(impact_metrics_take)
                    .cloned()
                    .collect()
            }
        })
        .filter(|b| !b.applications.is_empty() || !b.metrics.is_empty() || !b.impact_metrics.is_empty())
        .collect::<Vec<MetricsBatch>>()
}

impl MetricsCache {
    pub fn sink_impact_metrics(&self, impact_metrics: Vec<ImpactMetricEnv>) {
        for impact_metric in &impact_metrics { 
            let key = ImpactMetricsKey::from(impact_metric.clone());
            let existing_metrics = self.impact_metrics.get(&key).map(|m| m.value().clone()).unwrap_or_default();
            let mut aggregated_metrics: HashMap<String, ImpactMetricEnv> = HashMap::new();
            for metric in existing_metrics {
                let key = metric.impact_metric.name.clone();
                aggregated_metrics.insert(key, metric);
            }
            for mut metric in impact_metrics.clone() { 
                let mut samples_by_labels: HashMap<String, MetricSample> = HashMap::new();
                for sample in metric.impact_metric.samples {
                    let labels_key = Self::labels_to_key(&sample.labels);
                    if metric.impact_metric.r#type == "counter" {
                        if let Some(existing_sample) = samples_by_labels.get_mut(&labels_key) {
                            existing_sample.value += sample.value;
                        } else {
                            samples_by_labels.insert(labels_key, sample);
                        }
                    } else {
                        // For non-counter metrics (like gauge), last value wins
                        samples_by_labels.insert(labels_key, sample);
                    }
                }
                metric.impact_metric.samples = samples_by_labels.into_values().collect();
                if let Some(existing_metric) = aggregated_metrics.get_mut(&metric.impact_metric.name) {
                    Self::merge_two_metrics(existing_metric, metric);
                } else {
                    aggregated_metrics.insert(metric.impact_metric.name.clone(), metric);
                }
            }
            self.impact_metrics.insert(key, aggregated_metrics.into_values().collect());
        }
    }
    fn labels_to_key(labels: &Option<HashMap<String, String>>) -> String {
        match labels {
            Some(labels_map) => {
                let mut sorted_entries: Vec<(&String, &String)> = labels_map.iter().collect();
                sorted_entries.sort_by(|a, b| a.0.cmp(b.0));
                sorted_entries.iter()
                    .map(|(k, v)| format!("{}:{}", k, v))
                    .collect::<Vec<String>>()
                    .join(",")
            }
            None => "".to_string(),
        }
    }

    fn merge_two_metrics(existing_metric: &mut ImpactMetricEnv, new_metric: ImpactMetricEnv) {
        if existing_metric.impact_metric.r#type == "counter" && new_metric.impact_metric.r#type == "counter" {
            let mut samples_by_labels: HashMap<String, MetricSample> = HashMap::new();

            for sample in &existing_metric.impact_metric.samples {
                let labels_key = Self::labels_to_key(&sample.labels);
                samples_by_labels.insert(labels_key, sample.clone());
            }

            for sample in new_metric.impact_metric.samples {
                let labels_key = Self::labels_to_key(&sample.labels);
                if let Some(existing_sample) = samples_by_labels.get_mut(&labels_key) {
                    existing_sample.value += sample.value;
                } else {
                    samples_by_labels.insert(labels_key, sample);
                }
            }

            existing_metric.impact_metric.samples = samples_by_labels.into_values().collect();
        } else {
            let mut samples_by_labels: HashMap<String, MetricSample> = HashMap::new();

            for sample in &existing_metric.impact_metric.samples {
                let labels_key = Self::labels_to_key(&sample.labels);
                samples_by_labels.insert(labels_key, sample.clone());
            }

            for sample in new_metric.impact_metric.samples {
                let labels_key = Self::labels_to_key(&sample.labels);
                samples_by_labels.insert(labels_key, sample);
            }

            existing_metric.impact_metric.samples = samples_by_labels.into_values().collect();
        }
    }

    fn merge_impact_metrics(&self, metrics: Vec<ImpactMetricEnv>) -> Vec<ImpactMetricEnv> {
        let mut merged_metrics: HashMap<String, ImpactMetricEnv> = HashMap::new();

        for metric in metrics {
            if let Some(existing_metric) = merged_metrics.get_mut(&metric.impact_metric.name) {
                Self::merge_two_metrics(existing_metric, metric);
            } else {
                merged_metrics.insert(metric.impact_metric.name.clone(), metric);
            }
        }

        merged_metrics.into_values().collect()
    }

    pub fn get_metrics_by_environment(&self) -> HashMap<String, MetricsBatch> {
        let mut batches_by_environment = HashMap::new();

        let applications = self
            .applications
            .iter()
            .map(|e| e.value().clone())
            .collect::<Vec<ClientApplication>>();

        let mut all_environments = std::collections::HashSet::new();

        for entry in self.metrics.iter() {
            all_environments.insert(entry.value().environment.clone());
        }

        for entry in self.impact_metrics.iter() {
            all_environments.insert(entry.key().environment.clone());
        }

        let data = self
            .metrics
            .iter()
            .map(|e| e.value().clone())
            .collect::<Vec<ClientMetricsEnv>>();
        let metrics_by_env: HashMap<String, Vec<ClientMetricsEnv>> = data
            .into_iter()
            .into_group_map_by(|metric| metric.environment.clone());

        for environment in all_environments {
            let metrics = metrics_by_env.get(&environment).cloned().unwrap_or_default();

            let mut all_impact_metrics = Vec::new();
            for entry in self.impact_metrics.iter() {
                let key = entry.key();
                if key.environment == environment {
                    all_impact_metrics.extend(entry.value().clone());
                }
            }

            let merged_impact_metrics = self.merge_impact_metrics(all_impact_metrics);

            let batch = MetricsBatch {
                applications: applications.clone(),
                metrics,
                impact_metrics: merged_impact_metrics,
            };
            batches_by_environment.insert(environment, batch);
        }
        batches_by_environment
    }

    pub fn get_appropriately_sized_env_batches(&self, batch: &MetricsBatch) -> Vec<MetricsBatch> {
        for app in batch.applications.clone() {
            self.applications.remove(&ApplicationKey::from(app.clone()));
        }

        for impact_metric in batch.impact_metrics.clone() {
            self.impact_metrics.remove(&ImpactMetricsKey::from(impact_metric.clone()));
        }

        for metric in batch.metrics.clone() {
            self.metrics.remove(&MetricsKey::from(metric.clone()));
        }
        METRICS_SIZE_HISTOGRAM.observe(size_of_batch(batch) as f64);
        if sendable(batch) {
            vec![batch.clone()]
        } else {
            debug!(
                "We have {} applications and {} metrics",
                batch.applications.len(),
                batch.metrics.len()
            );
            cut_into_sendable_batches(batch.clone())
        }
    }
    /// This is a destructive call. We'll remove all metrics that is due for posting
    /// Called from [crate::http::background_send_metrics::send_metrics_task] which will reinsert on 5xx server failures, but leave 413 and 400 failures on the floor
    pub fn get_appropriately_sized_batches(&self) -> Vec<MetricsBatch> {
        let impact_keys: Vec<ImpactMetricsKey> = self
            .impact_metrics
            .iter()
            .map(|e| e.key().clone())
            .collect();

        let mut all_impact_metrics = Vec::new();
        for entry in self.impact_metrics.iter() {
            all_impact_metrics.extend(entry.value().clone());
        }

        let merged_impact_metrics = self.merge_impact_metrics(all_impact_metrics);

        let batch = MetricsBatch {
            applications: self
                .applications
                .iter()
                .map(|e| e.value().clone())
                .collect(),
            metrics: self
                .metrics
                .iter()
                .map(|e| e.value().clone())
                .filter(|m| m.yes > 0 || m.no > 0) // Makes sure that we only return buckets that have values. We should have a test for this :P
                .collect(),
            impact_metrics: merged_impact_metrics,
        };
        for app in batch.applications.clone() {
            self.applications.remove(&ApplicationKey::from(app.clone()));
        }

        for key in &impact_keys {
            self.impact_metrics.remove(key);
        }

        for metric in batch.metrics.clone() {
            self.metrics.remove(&MetricsKey::from(metric.clone()));
        }
        METRICS_SIZE_HISTOGRAM.observe(size_of_batch(&batch) as f64);
        if sendable(&batch) {
            vec![batch]
        } else {
            debug!(
                "We have {} applications and {} metrics",
                batch.applications.len(),
                batch.metrics.len()
            );
            cut_into_sendable_batches(batch)
        }
    }

    pub fn reinsert_batch(&self, batch: MetricsBatch) {
        for application in batch.applications {
            self.register_application(application);
        }

        self.sink_impact_metrics(batch.impact_metrics.clone());

        self.sink_metrics(&batch.metrics);
    }

    pub fn sink_bulk_metrics(&self, metrics: BatchMetricsRequestBody, connect_via: &ConnectVia) {
        for application in metrics.applications {
            self.register_application(
                application.connect_via(&connect_via.app_name, &connect_via.instance_id),
            )
        }

        // TODO: sink impact metrics

        self.sink_metrics(&metrics.metrics)
    }

    pub fn reset_metrics(&self) {
        self.applications.clear();
        self.metrics.clear();
        self.impact_metrics.clear();
    }

    pub fn register_application(&self, application: ClientApplication) {
        self.applications
            .insert(ApplicationKey::from(application.clone()), application);
    }

    pub fn sink_metrics(&self, metrics: &[ClientMetricsEnv]) {
        debug!("Sinking {} metrics", metrics.len());
        for metric in metrics.iter() {
            FEATURE_TOGGLE_USAGE_TOTAL
                .with_label_values(&[
                    metric.app_name.clone(),
                    metric.feature_name.clone(),
                    "true".to_string(),
                ])
                .inc_by(metric.yes as u64);
            FEATURE_TOGGLE_USAGE_TOTAL
                .with_label_values(&[
                    metric.app_name.clone(),
                    metric.feature_name.clone(),
                    "false".to_string(),
                ])
                .inc_by(metric.no as u64);
            self.metrics
                .entry(MetricsKey {
                    app_name: metric.app_name.clone(),
                    feature_name: metric.feature_name.clone(),
                    timestamp: metric.timestamp,
                    environment: metric.environment.clone(),
                })
                .and_modify(|feature_stats| {
                    feature_stats.yes += metric.yes;
                    feature_stats.no += metric.no;
                    metric.variants.iter().for_each(|(k, added_count)| {
                        feature_stats
                            .variants
                            .entry(k.clone())
                            .and_modify(|count| {
                                *count += added_count;
                            })
                            .or_insert(*added_count);
                    });
                })
                .or_insert_with(|| metric.clone());
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::types::{TokenType, TokenValidationStatus};
    use chrono::{DateTime, Utc};
    use std::collections::HashMap;
    use std::str::FromStr;
    use test_case::test_case;
    use unleash_types::client_metrics::SdkType::Backend;
    use unleash_types::client_metrics::{
        ClientMetricsEnv, ConnectVia, ConnectViaBuilder, MetricsMetadata,
    };

    #[test]
    fn cache_aggregates_data_correctly() {
        let cache = MetricsCache::default();

        let base_metric = ClientMetricsEnv {
            app_name: "some-app".into(),
            feature_name: "some-feature".into(),
            environment: "development".into(),
            timestamp: DateTime::parse_from_rfc3339("1867-11-07T12:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            yes: 1,
            no: 0,
            variants: HashMap::new(),
            metadata: MetricsMetadata {
                platform_name: None,
                platform_version: None,
                sdk_version: None,
                sdk_type: None,
                yggdrasil_version: None,
            },
        };

        let metrics = vec![
            ClientMetricsEnv {
                ..base_metric.clone()
            },
            ClientMetricsEnv { ..base_metric },
        ];

        cache.sink_metrics(&metrics);

        let found_metric = cache
            .metrics
            .get(&MetricsKey {
                app_name: "some-app".into(),
                feature_name: "some-feature".into(),
                timestamp: DateTime::parse_from_rfc3339("1867-11-07T12:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
                environment: "development".into(),
            })
            .unwrap();

        let expected = ClientMetricsEnv {
            app_name: "some-app".into(),
            feature_name: "some-feature".into(),
            environment: "development".into(),
            timestamp: DateTime::parse_from_rfc3339("1867-11-07T12:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            yes: 2,
            no: 0,
            variants: HashMap::new(),
            metadata: MetricsMetadata {
                platform_name: None,
                platform_version: None,
                sdk_version: None,
                sdk_type: None,
                yggdrasil_version: None,
            },
        };

        assert_eq!(found_metric.yes, expected.yes);
        assert_eq!(found_metric.yes, 2);
        assert_eq!(found_metric.no, 0);
        assert_eq!(found_metric.no, expected.no);
    }

    #[test]
    fn cache_aggregates_data_correctly_across_date_boundaries() {
        let cache = MetricsCache::default();
        let a_long_time_ago = DateTime::parse_from_rfc3339("1867-11-07T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let hundred_years_later = DateTime::parse_from_rfc3339("1967-11-07T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        let base_metric = ClientMetricsEnv {
            app_name: "some-app".into(),
            feature_name: "some-feature".into(),
            environment: "development".into(),
            timestamp: a_long_time_ago,
            yes: 1,
            no: 0,
            variants: HashMap::new(),
            metadata: MetricsMetadata {
                platform_name: None,
                platform_version: None,
                sdk_version: None,
                sdk_type: None,
                yggdrasil_version: None,
            },
        };

        let metrics = vec![
            ClientMetricsEnv {
                timestamp: hundred_years_later,
                ..base_metric.clone()
            },
            ClientMetricsEnv {
                ..base_metric.clone()
            },
            ClientMetricsEnv { ..base_metric },
        ];

        cache.sink_metrics(&metrics);

        let old_metric = cache
            .metrics
            .get(&MetricsKey {
                app_name: "some-app".into(),
                feature_name: "some-feature".into(),
                environment: "development".into(),
                timestamp: a_long_time_ago,
            })
            .unwrap();

        let old_expectation = ClientMetricsEnv {
            app_name: "some-app".into(),
            feature_name: "some-feature".into(),
            environment: "development".into(),
            timestamp: a_long_time_ago,
            yes: 2,
            no: 0,
            variants: HashMap::new(),
            metadata: MetricsMetadata {
                platform_name: None,
                platform_version: None,
                sdk_version: None,
                sdk_type: None,
                yggdrasil_version: None,
            },
        };

        let new_metric = cache
            .metrics
            .get(&MetricsKey {
                app_name: "some-app".into(),
                feature_name: "some-feature".into(),
                environment: "development".into(),
                timestamp: hundred_years_later,
            })
            .unwrap();

        let new_expectation = ClientMetricsEnv {
            app_name: "some-app".into(),
            feature_name: "some-feature".into(),
            environment: "development".into(),
            timestamp: hundred_years_later,
            yes: 1,
            no: 0,
            variants: HashMap::new(),
            metadata: MetricsMetadata {
                platform_name: None,
                platform_version: None,
                sdk_version: None,
                sdk_type: None,
                yggdrasil_version: None,
            },
        };

        assert_eq!(cache.metrics.len(), 2);

        assert_eq!(old_metric.yes, old_expectation.yes);
        assert_eq!(old_metric.yes, 2);
        assert_eq!(old_metric.no, 0);
        assert_eq!(old_metric.no, old_expectation.no);

        assert_eq!(new_metric.yes, new_expectation.yes);
        assert_eq!(new_metric.yes, 1);
        assert_eq!(new_metric.no, 0);
        assert_eq!(new_metric.no, new_expectation.no);
    }

    #[test]
    fn cache_clears_metrics_correctly() {
        let cache = MetricsCache::default();
        let time_stamp = DateTime::parse_from_rfc3339("1867-11-07T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        let base_metric = ClientMetricsEnv {
            app_name: "some-app".into(),
            feature_name: "some-feature".into(),
            environment: "development".into(),
            timestamp: time_stamp,
            yes: 1,
            no: 0,
            variants: HashMap::new(),
            metadata: MetricsMetadata {
                platform_name: None,
                platform_version: None,
                sdk_version: None,
                sdk_type: None,
                yggdrasil_version: None,
            },
        };

        let metrics = vec![
            ClientMetricsEnv {
                ..base_metric.clone()
            },
            ClientMetricsEnv { ..base_metric },
        ];

        cache.sink_metrics(&metrics);
        assert!(!cache.metrics.is_empty());
        cache.reset_metrics();
        assert!(cache.metrics.is_empty());
    }

    #[test]
    fn adding_another_connection_link_works() {
        let client_application = ClientApplication {
            app_name: "tests_help".into(),
            connect_via: None,
            environment: Some("development".into()),
            projects: None,
            instance_id: Some("test".into()),
            connection_id: Some("test".into()),
            interval: 60,
            started: Default::default(),
            strategies: vec![],
            metadata: MetricsMetadata {
                platform_name: None,
                platform_version: None,
                sdk_version: None,
                sdk_type: None,
                yggdrasil_version: None,
            },
        };
        let connected_via_test_instance = client_application.connect_via("test", "instance");
        let connected_via_edge_as_well = connected_via_test_instance.connect_via("edge", "edgeid");
        assert_eq!(
            connected_via_test_instance.connect_via.unwrap(),
            vec![ConnectVia {
                app_name: "test".into(),
                instance_id: "instance".into()
            }]
        );
        assert_eq!(
            connected_via_edge_as_well.connect_via.unwrap(),
            vec![
                ConnectVia {
                    app_name: "test".into(),
                    instance_id: "instance".into()
                },
                ConnectVia {
                    app_name: "edge".into(),
                    instance_id: "edgeid".into()
                }
            ]
        )
    }

    #[test_case(10, 100, 1; "10 apps 100 toggles. Will not be split")]
    #[test_case(1, 10000, 27; "1 app 10k toggles, will be split into 27 batches")]
    #[test_case(1000, 1000, 8; "1000 apps 1000 toggles, will be split into 8 batches")]
    #[test_case(500, 5000, 16; "500 apps 5000 toggles, will be split into 16 batches")]
    #[test_case(5000, 1, 20; "5000 apps 1 metric will be split")]
    fn splits_successfully_into_sendable_chunks(apps: u64, toggles: u64, batch_count: usize) {
        let apps: Vec<ClientApplication> = (1..=apps)
            .map(|app_id| ClientApplication {
                app_name: format!("app_name_{}", app_id),
                environment: Some("development".into()),
                projects: Some(vec![]),
                instance_id: Some(format!("instance-{}", app_id)),
                connection_id: Some(format!("connection-{}", app_id)),
                interval: 10,
                connect_via: Some(vec![ConnectVia {
                    app_name: "edge".into(),
                    instance_id: "some-instance-id".into(),
                }]),
                started: DateTime::parse_from_rfc3339("1867-11-07T12:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
                strategies: vec![],
                metadata: MetricsMetadata {
                    platform_name: None,
                    platform_version: None,
                    sdk_version: Some("some-test-sdk".into()),
                    sdk_type: Some(Backend),
                    yggdrasil_version: None,
                },
            })
            .collect();

        let toggles: Vec<ClientMetricsEnv> = (1..=toggles)
            .map(|toggle_id| ClientMetricsEnv {
                app_name: format!("app_name_{}", toggle_id),
                feature_name: format!("toggle-{}", toggle_id),
                environment: "development".into(),
                timestamp: DateTime::parse_from_rfc3339("1867-11-07T12:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
                yes: 1,
                no: 1,
                variants: HashMap::new(),
                metadata: MetricsMetadata {
                    platform_name: None,
                    platform_version: None,
                    sdk_version: None,
                    sdk_type: None,
                    yggdrasil_version: None,
                },
            })
            .collect();

        let cache = MetricsCache::default();
        for app in apps.clone() {
            cache.applications.insert(
                ApplicationKey {
                    app_name: app.app_name.clone(),
                    instance_id: app.instance_id.clone().unwrap_or_else(|| "unknown".into()),
                },
                app,
            );
        }
        cache.sink_metrics(&toggles);
        let batches = cache.get_appropriately_sized_batches();

        assert_eq!(batches.len(), batch_count);
        assert!(batches.iter().all(sendable));
        // Check that we have no duplicates
        let applications_sent_count = batches.iter().flat_map(|b| b.applications.clone()).count();

        assert_eq!(applications_sent_count, apps.len());

        let metrics_sent_count = batches.iter().flat_map(|b| b.metrics.clone()).count();
        assert_eq!(metrics_sent_count, toggles.len());
    }

    #[test]
    fn getting_unsent_metrics_filters_out_metrics_with_no_counters() {
        let cache = MetricsCache::default();

        let base_metric = ClientMetricsEnv {
            app_name: "some-app".into(),
            feature_name: "some-feature".into(),
            environment: "development".into(),
            timestamp: DateTime::parse_from_rfc3339("1867-11-07T12:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            yes: 0,
            no: 0,
            variants: HashMap::new(),
            metadata: MetricsMetadata {
                platform_name: None,
                platform_version: None,
                sdk_version: None,
                sdk_type: None,
                yggdrasil_version: None,
            },
        };

        let metrics = vec![
            ClientMetricsEnv {
                ..base_metric.clone()
            },
            ClientMetricsEnv { ..base_metric },
        ];

        cache.sink_metrics(&metrics);
        let metrics_batch = cache.get_appropriately_sized_batches();
        assert_eq!(metrics_batch.len(), 1);
        assert!(metrics_batch.first().unwrap().metrics.is_empty());
    }

    #[test]
    pub fn register_bulk_metrics_filters_metrics_based_on_environment_in_token() {
        let metrics_cache = MetricsCache::default();
        let connect_via = ConnectViaBuilder::default()
            .app_name("edge_bulk_metrics".into())
            .instance_id("sometest".into())
            .build()
            .unwrap();
        let mut edge_token_with_development =
            EdgeToken::from_str("*:development.randomstring").unwrap();
        edge_token_with_development.status = TokenValidationStatus::Validated;
        edge_token_with_development.token_type = Some(TokenType::Client);
        let metrics = BatchMetricsRequestBody {
            applications: vec![],
            metrics: vec![
                ClientMetricsEnv {
                    feature_name: "feature_one".into(),
                    app_name: "my_app".into(),
                    environment: "development".into(),
                    timestamp: Utc::now(),
                    yes: 50,
                    no: 10,
                    variants: Default::default(),
                    metadata: MetricsMetadata {
                        platform_name: None,
                        platform_version: None,
                        sdk_version: None,
                        sdk_type: None,
                        yggdrasil_version: None,
                    },
                },
                ClientMetricsEnv {
                    feature_name: "feature_two".to_string(),
                    app_name: "other_app".to_string(),
                    environment: "production".to_string(),
                    timestamp: Default::default(),
                    yes: 50,
                    no: 10,
                    variants: Default::default(),
                    metadata: MetricsMetadata {
                        platform_name: None,
                        platform_version: None,
                        sdk_version: None,
                        sdk_type: None,
                        yggdrasil_version: None,
                    },
                },
            ],
            impact_metrics: None,
        };
        register_bulk_metrics(
            &metrics_cache,
            &connect_via,
            &edge_token_with_development,
            metrics,
        );
        assert_eq!(metrics_cache.metrics.len(), 1);
    }

    #[test]
    pub fn metrics_will_be_gathered_per_environment() {
        let metrics = vec![
            ClientMetricsEnv {
                feature_name: "feature_one".into(),
                app_name: "my_app".into(),
                environment: "development".into(),
                timestamp: Utc::now(),
                yes: 50,
                no: 10,
                variants: Default::default(),
                metadata: MetricsMetadata {
                    platform_name: None,
                    platform_version: None,
                    sdk_version: None,
                    sdk_type: None,
                    yggdrasil_version: None,
                },
            },
            ClientMetricsEnv {
                feature_name: "feature_two".to_string(),
                app_name: "other_app".to_string(),
                environment: "production".to_string(),
                timestamp: Default::default(),
                yes: 50,
                no: 10,
                variants: Default::default(),
                metadata: MetricsMetadata {
                    platform_name: None,
                    platform_version: None,
                    sdk_version: None,
                    sdk_type: None,
                    yggdrasil_version: None,
                },
            },
        ];
        let cache = MetricsCache::default();
        cache.sink_metrics(&metrics);
        let metrics_by_env_map = cache.get_metrics_by_environment();
        assert_eq!(metrics_by_env_map.len(), 2);
        assert!(metrics_by_env_map.contains_key("development"));
        assert!(metrics_by_env_map.contains_key("production"));
    }

    fn create_sample(value: f64, labels: HashMap<String, String>) -> MetricSample {
        MetricSample {
            value,
            labels: Some(labels),
        }
    }

    fn create_impact_metric(name: &str, r#type: &str, samples: Vec<MetricSample>) -> ImpactMetric {
        ImpactMetric {
            name: name.into(),
            help: format!("Test {} metric", r#type).into(),
            r#type: r#type.into(),
            samples,
        }
    }

    fn create_test_labels(key: &str, value: &str) -> HashMap<String, String> {
        HashMap::from([(key.into(), value.into())])
    }

    #[test]
    pub fn sink_impact_metrics_aggregates_correctly() {
        // Setup
        let cache = MetricsCache::default();
        let app = "test_app";
        let env = "test_env";
        let test_key = ImpactMetricsKey { app_name: app.into(), environment: env.into() };

        let labels1 = create_test_labels("label1", "value1");
        let labels2 = create_test_labels("label1", "different");

        let counter_metrics = vec![
            create_impact_metric("test_counter", "counter", vec![
                create_sample(1.0, labels1.clone()),
                create_sample(2.0, labels1.clone()),
                create_sample(3.0, labels2.clone()),
            ]),
        ];

        let gauge_metrics = vec![
            create_impact_metric("test_gauge", "gauge", vec![
                create_sample(1.0, labels1.clone()),
                create_sample(2.0, labels1.clone()),
            ]),
        ];

        cache.sink_impact_metrics(
            convert_to_impact_metrics_env(counter_metrics, app.into(), env.into()),
        );
        cache.sink_impact_metrics(
            convert_to_impact_metrics_env(gauge_metrics, app.into(), env.into()),
        );

        let aggregated_metrics = cache.impact_metrics.get(&test_key).unwrap();
        let counter = aggregated_metrics.value().iter()
            .find(|m| m.impact_metric.name == "test_counter")
            .unwrap();

        let value1_sample = counter.impact_metric.samples.iter()
            .find(|s| s.labels.as_ref().unwrap().get("label1") == Some(&"value1".into()))
            .unwrap();
        assert_eq!(value1_sample.value, 3.0, "Counter values should be summed");

        let gauge = aggregated_metrics.value().iter()
            .find(|m| m.impact_metric.name == "test_gauge")
            .unwrap();
        assert_eq!(gauge.impact_metric.samples[0].value, 2.0, "Gauge should have the last value");
    }

    #[test]
    pub fn merge_impact_metrics_from_different_apps() {
        let cache = MetricsCache::default();
        let env = "default";

        let app1 = "app1";
        let app1_key = ImpactMetricsKey { app_name: app1.into(), environment: env.into() };
        let app1_labels = HashMap::from([("appName".into(), "my-application-1".into())]);
        let app1_metrics = vec![
            create_impact_metric("test", "counter", vec![
                create_sample(10.0, app1_labels),
            ]),
        ];

        let app2 = "app2";
        let app2_key = ImpactMetricsKey { app_name: app2.into(), environment: env.into() };
        let app2_labels = HashMap::from([("appName".into(), "my-application-2".into())]);
        let app2_metrics = vec![
            create_impact_metric("test", "counter", vec![
                create_sample(1.0, app2_labels),
            ]),
        ];

        cache.impact_metrics.insert(app1_key, convert_to_impact_metrics_env(app1_metrics, app1.into(), env.into()));
        cache.impact_metrics.insert(app2_key, convert_to_impact_metrics_env(app2_metrics, app2.into(), env.into()));

        let mut all_impact_metrics = Vec::new();
        for entry in cache.impact_metrics.iter() {
            all_impact_metrics.extend(entry.value().clone());
        }
        let merged_impact_metrics = cache.merge_impact_metrics(all_impact_metrics);

        assert_eq!(merged_impact_metrics.len(), 1, "Should have one merged metric");
        let test_metric = &merged_impact_metrics[0];

        let app1_value = test_metric.impact_metric.samples.iter()
            .find(|s| s.labels.as_ref().unwrap().get("appName") == Some(&"my-application-1".into()))
            .unwrap().value;
        let app2_value = test_metric.impact_metric.samples.iter()
            .find(|s| s.labels.as_ref().unwrap().get("appName") == Some(&"my-application-2".into()))
            .unwrap().value;

        assert_eq!(app1_value, 10.0, "App1 sample value should be preserved");
        assert_eq!(app2_value, 1.0, "App2 sample value should be preserved");
    }

    fn create_client_metrics(app_name: &str, feature_name: &str, environment: &str, yes: u32, no: u32) -> ClientMetricsEnv {
        ClientMetricsEnv {
            app_name: app_name.into(),
            feature_name: feature_name.into(),
            environment: environment.into(),
            timestamp: Utc::now(),
            yes,
            no,
            variants: HashMap::new(),
            metadata: MetricsMetadata {
                platform_name: None,
                platform_version: None,
                sdk_version: None,
                sdk_type: None,
                yggdrasil_version: None,
            },
        }
    }

    fn create_and_sink_impact_metrics(cache: &MetricsCache, app_name: &str, env: &str, metric_name: &str, metric_type: &str, value: f64) {
        let labels = HashMap::from([("env".into(), env.into())]);
        let impact_metrics = vec![
            ImpactMetricEnv::new(
                create_impact_metric(metric_name, metric_type, vec![
                    create_sample(value, labels)
                ]),
                app_name.into(),
                env.into(),
            )
        ];
        cache.sink_impact_metrics(impact_metrics);
    }

    #[test]
    pub fn get_metrics_by_environment_handles_metrics_and_impact_metrics_independently() {
        let cache = MetricsCache::default();

        // 1. Development: only regular metrics
        let dev_env = "development";
        let dev_app = "dev-app";
        cache.sink_metrics(&[create_client_metrics(dev_app, "feature", dev_env, 5, 2)]);

        // 2. Production: only impact metrics
        let prod_env = "production";
        let prod_app = "prod-app";
        create_and_sink_impact_metrics(&cache, prod_app, prod_env, "counter", "counter", 10.0);

        // 3. Staging: both regular and impact metrics
        let staging_env = "staging";
        let staging_app = "staging-app";
        cache.sink_metrics(&[create_client_metrics(staging_app, "feature", staging_env, 3, 1)]);
        create_and_sink_impact_metrics(&cache, staging_app, staging_env, "gauge", "gauge", 42.0);

        let batches = cache.get_metrics_by_environment();

        let dev_batch = &batches[dev_env];
        assert_eq!(dev_batch.metrics.len(), 1);
        assert_eq!(dev_batch.impact_metrics.len(), 0);

        let prod_batch = &batches[prod_env];
        assert_eq!(prod_batch.metrics.len(), 0);
        assert_eq!(prod_batch.impact_metrics[0].impact_metric.name, "counter");

        let staging_batch = &batches[staging_env];
        assert_eq!(staging_batch.metrics.len(), 1);
        assert_eq!(staging_batch.impact_metrics[0].impact_metric.name, "gauge");
    }
}
