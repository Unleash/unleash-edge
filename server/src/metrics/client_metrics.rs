use crate::metrics::client_impact_metrics::{
    ImpactMetricsKey, convert_to_impact_metrics_env, merge_impact_metrics,
};
use crate::metrics::metric_batching::{cut_into_sendable_batches, sendable, size_of_batch};
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
use tracing::debug;
use unleash_types::client_metrics::SdkType::Backend;
use unleash_types::client_metrics::{
    ClientApplication, ClientMetrics, ClientMetricsEnv, ConnectVia, ImpactMetricEnv,
    MetricsMetadata,
};
use utoipa::ToSchema;

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

#[derive(Debug, Clone, Eq, Deserialize, Serialize, ToSchema)]
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
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        rename = "impactMetrics"
    )]
    pub impact_metrics: Vec<ImpactMetricEnv>,
}

#[derive(Default, Debug)]
pub struct MetricsCache {
    pub(crate) applications: DashMap<ApplicationKey, ClientApplication>,
    pub(crate) metrics: DashMap<MetricsKey, ClientMetricsEnv>,
    pub(crate) impact_metrics: DashMap<ImpactMetricsKey, Vec<ImpactMetricEnv>>,
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
        let impact_metrics_env =
            convert_to_impact_metrics_env(impact_metrics, metrics.app_name.clone(), environment);
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

impl MetricsCache {
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
            let metrics = metrics_by_env
                .get(&environment)
                .cloned()
                .unwrap_or_default();

            let mut all_impact_metrics = Vec::new();
            for entry in self.impact_metrics.iter() {
                let key = entry.key();
                if key.environment == environment {
                    all_impact_metrics.extend(entry.value().clone());
                }
            }

            let batch = MetricsBatch {
                applications: applications.clone(),
                metrics,
                impact_metrics: all_impact_metrics,
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
            self.impact_metrics
                .remove(&ImpactMetricsKey::from(&impact_metric));
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

        let merged_impact_metrics = merge_impact_metrics(all_impact_metrics);

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
}
