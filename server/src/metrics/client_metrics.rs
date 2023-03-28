use chrono::{DateTime, Utc};
use dashmap::DashMap;
use lazy_static::lazy_static;
use prometheus::{register_histogram, Histogram};
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};
use tracing::{debug, instrument};
use unleash_types::client_metrics::{ClientApplication, ClientMetricsEnv};
pub const UPSTREAM_MAX_BODY_SIZE: usize = 100 * 1024;
pub const BATCH_BODY_SIZE: usize = 95 * 1024;

lazy_static! {
    pub static ref METRICS_SIZE_HISTOGRAM: Histogram = register_histogram!(
        "metrics_size_in_bytes",
        "Size of metrics when posting",
        vec![1000.0, 10000.0, 20000.0, 50000.0, 75000.0, 100000.0, 250000.0, 500000.0, 1000000.0]
    )
    .unwrap();
}

#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub struct ApplicationKey {
    pub app_name: String,
    pub instance_id: String,
}

impl ApplicationKey {
    pub fn from_app_name(app_name: String) -> Self {
        Self {
            app_name,
            instance_id: ulid::Ulid::new().to_string(),
        }
    }
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

#[derive(Debug, Clone, Eq)]
pub struct MetricsKey {
    pub app_name: String,
    pub feature_name: String,
    pub environment: String,
    pub timestamp: DateTime<Utc>,
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

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct MetricsBatch {
    pub applications: Vec<ClientApplication>,
    pub metrics: Vec<ClientMetricsEnv>,
}

#[derive(Default, Debug)]
pub struct MetricsCache {
    pub applications: DashMap<ApplicationKey, ClientApplication>,
    pub metrics: DashMap<MetricsKey, ClientMetricsEnv>,
}

pub fn size_of_batch(batch: &MetricsBatch) -> usize {
    serde_json::to_string(batch)
        .map(|s| s.as_bytes().len())
        .unwrap_or(0)
}

pub fn sendable(batch: &MetricsBatch) -> bool {
    size_of_batch(batch) < UPSTREAM_MAX_BODY_SIZE
}

#[instrument(skip(batch))]
pub fn cut_into_sendable_batches(batch: MetricsBatch) -> Vec<MetricsBatch> {
    let batch_count = (size_of_batch(&batch) / BATCH_BODY_SIZE) + 1;
    let apps_count = batch.applications.len();
    let apps_per_batch = apps_count / batch_count;

    let metrics_count = batch.metrics.len();
    let metrics_per_batch = metrics_count / batch_count;

    debug!("Batch count: {batch_count}. Apps per batch: {apps_per_batch}, Metrics per batch: {metrics_per_batch}");
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
            }
        })
        .filter(|b| !b.applications.is_empty() || !b.metrics.is_empty())
        .collect::<Vec<MetricsBatch>>()
}

impl MetricsCache {
    /// This is a destructive call. We'll remove all metrics that is due for posting
    /// Called from [crate::http::background_send_metrics::send_metrics_task] which will reinsert on 5xx server failures, but leave 413 and 400 failures on the floor
    pub fn get_appropriately_sized_batches(&self) -> Vec<MetricsBatch> {
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
        };
        for app in batch.applications.clone() {
            self.applications.remove(&ApplicationKey::from(app.clone()));
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
        self.sink_metrics(&batch.metrics);
    }

    pub fn reset_metrics(&self) {
        self.applications.clear();
        self.metrics.clear();
    }

    pub fn register_application(&self, application: ClientApplication) {
        self.applications
            .insert(ApplicationKey::from(application.clone()), application);
    }

    pub fn sink_metrics(&self, metrics: &[ClientMetricsEnv]) {
        debug!("Sinking {} metrics", metrics.len());
        for metric in metrics.iter() {
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
    use chrono::{DateTime, Utc};
    use std::collections::HashMap;
    use test_case::test_case;
    use unleash_types::client_metrics::{ClientMetricsEnv, ConnectVia};

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
            instance_id: Some("test".into()),
            interval: 60,
            sdk_version: None,
            started: Default::default(),
            strategies: vec![],
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
    #[test_case(1, 10000, 16; "1 app 10k toggles, will be split into 16 batches")]
    #[test_case(1000, 1000, 4; "1000 apps 1000 toggles, will be split into 4 batches")]
    #[test_case(500, 5000, 10; "500 apps 5000 toggles, will be split into 10 batches")]
    #[test_case(5000, 1, 14; "5000 apps 1 metric will be split")]
    fn splits_successfully_into_sendable_chunks(apps: u64, toggles: u64, batch_count: usize) {
        let apps: Vec<ClientApplication> = (1..=apps)
            .map(|app_id| ClientApplication {
                app_name: format!("app_name_{}", app_id),
                environment: Some("development".into()),
                instance_id: Some(format!("instance-{}", app_id)),
                interval: 10,
                connect_via: Some(vec![ConnectVia {
                    app_name: "edge".into(),
                    instance_id: "some-instance-id".into(),
                }]),
                sdk_version: Some("some-test-sdk".into()),
                started: DateTime::parse_from_rfc3339("1867-11-07T12:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
                strategies: vec![],
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
        assert!(metrics_batch.get(0).unwrap().metrics.is_empty());
    }
}
