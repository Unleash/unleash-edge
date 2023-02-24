use chrono::{DateTime, Utc};
use dashmap::DashMap;
use std::hash::{Hash, Hasher};
use unleash_types::client_metrics::{ClientApplication, ClientMetricsEnv};
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

#[derive(Debug, Clone, Eq)]
pub struct MetricsKey {
    pub app_name: String,
    pub feature_name: String,
    pub timestamp: DateTime<Utc>,
}

impl Hash for MetricsKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.app_name.hash(state);
        self.feature_name.hash(state);
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
            && self_hour_bin == other_hour_bin
    }
}

pub struct MetricsBatch {
    pub applications: Vec<ClientApplication>,
    pub metrics: Vec<ClientMetricsEnv>,
}

#[derive(Default, Debug)]
pub struct MetricsCache {
    pub applications: DashMap<ApplicationKey, ClientApplication>,
    pub metrics: DashMap<MetricsKey, ClientMetricsEnv>,
}

impl MetricsCache {
    pub fn get_unsent_metrics(&self) -> MetricsBatch {
        MetricsBatch {
            applications: self
                .applications
                .iter()
                .map(|e| e.value().clone())
                .collect(),
            metrics: self.metrics.iter().map(|e| e.value().clone()).collect(),
        }
    }
    pub fn reset_metrics(&self) {
        self.applications.clear();
        self.metrics.clear();
    }

    pub fn sink_metrics(&self, metrics: &[ClientMetricsEnv]) {
        for metric in metrics.iter() {
            self.metrics
                .entry(MetricsKey {
                    app_name: metric.app_name.clone(),
                    feature_name: metric.feature_name.clone(),
                    timestamp: metric.timestamp,
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
    use std::collections::HashMap;

    use chrono::{DateTime, Utc};
    use unleash_types::client_metrics::ClientMetricsEnv;

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
}
