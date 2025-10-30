use itertools::Itertools;
use std::collections::{BTreeMap, HashMap};
use unleash_edge_types::metrics::{ImpactMetricsKey, MetricsCache};
use unleash_types::MergeMut;
use unleash_types::client_metrics::{ImpactMetric, ImpactMetricEnv};

pub fn convert_to_impact_metrics_env(
    metrics: Vec<ImpactMetric>,
    app_name: String,
    environment: String,
) -> Vec<ImpactMetricEnv> {
    metrics
        .into_iter()
        .map(|metric| ImpactMetricEnv::new(metric, app_name.clone(), environment.clone()))
        .collect()
}

fn group_by_key(
    impact_metrics: Vec<ImpactMetricEnv>,
) -> HashMap<ImpactMetricsKey, Vec<ImpactMetricEnv>> {
    impact_metrics
        .into_iter()
        .chunk_by(|m| ImpactMetricsKey::from(m))
        .into_iter()
        .map(|(k, group)| (k, group.collect()))
        .collect()
}

fn index_by_name(metrics: Vec<ImpactMetricEnv>) -> HashMap<String, ImpactMetricEnv> {
    metrics
        .into_iter()
        .map(|metric| (metric.impact_metric.name().to_string(), metric))
        .collect()
}

fn reduce_metrics_samples(metric: ImpactMetricEnv) -> ImpactMetricEnv {
    let empty_impact_metric = match &metric.impact_metric {
        ImpactMetric::Counter { name, help, .. } => ImpactMetric::Counter {
            name: name.clone(),
            help: help.clone(),
            samples: vec![],
        },
        ImpactMetric::Gauge { name, help, .. } => ImpactMetric::Gauge {
            name: name.clone(),
            help: help.clone(),
            samples: vec![],
        },
        ImpactMetric::Histogram { name, help, .. } => ImpactMetric::Histogram {
            name: name.clone(),
            help: help.clone(),
            samples: vec![],
        },
    };

    let mut empty_metric = ImpactMetricEnv::new(
        empty_impact_metric,
        metric.app_name.clone(),
        metric.environment.clone(),
    );

    empty_metric.merge(metric);
    empty_metric
}

pub fn merge_impact_metrics(metrics: Vec<ImpactMetricEnv>) -> Vec<ImpactMetricEnv> {
    if metrics.is_empty() {
        return Vec::new();
    }

    let mut merged_metrics: HashMap<String, ImpactMetricEnv> =
        HashMap::with_capacity(metrics.len());

    for metric in metrics {
        let metric_name = metric.impact_metric.name().to_string();

        if let Some(existing_metric) = merged_metrics.get_mut(&metric_name) {
            existing_metric.merge(metric);
        } else {
            merged_metrics.insert(metric_name, metric);
        }
    }

    merged_metrics.into_values().collect()
}

pub fn sink_impact_metrics(cache: &MetricsCache, impact_metrics: Vec<ImpactMetricEnv>) {
    let metrics_by_key = group_by_key(impact_metrics);

    for (key, metrics) in metrics_by_key {
        let existing_metrics = cache
            .impact_metrics
            .get(&key)
            .map(|m| m.value().clone())
            .unwrap_or_default();

        let mut aggregated_metrics = index_by_name(existing_metrics);

        for metric in metrics {
            let reduced_metric = reduce_metrics_samples(metric);

            if let Some(existing_metric) =
                aggregated_metrics.get_mut(reduced_metric.impact_metric.name())
            {
                existing_metric.merge(reduced_metric);
            } else {
                aggregated_metrics.insert(
                    reduced_metric.impact_metric.name().to_string(),
                    reduced_metric,
                );
            }
        }
        let layered_metrics = aggregated_metrics
            .into_values()
            .map(|mut metric| {
                match &mut metric.impact_metric {
                    ImpactMetric::Counter { samples, .. } | ImpactMetric::Gauge { samples, .. } => {
                        for sample in samples {
                            if let Some(labels) = &mut sample.labels {
                                labels.insert("origin".to_string(), "edge".into());
                            } else {
                                sample.labels =
                                    Some(BTreeMap::from([("origin".into(), "edge".into())]));
                            }
                        }
                    }
                    ImpactMetric::Histogram { samples, .. } => {
                        for sample in samples {
                            if let Some(labels) = &mut sample.labels {
                                labels.insert("origin".to_string(), "edge".into());
                            } else {
                                sample.labels =
                                    Some(BTreeMap::from([("origin".into(), "edge".into())]));
                            }
                        }
                    }
                }
                metric
            })
            .collect::<Vec<_>>();

        cache.impact_metrics.insert(key, layered_metrics);
    }
}

#[cfg(test)]
mod test {
    use crate::client_impact_metrics::{
        convert_to_impact_metrics_env, merge_impact_metrics, sink_impact_metrics,
    };
    use crate::client_metrics::{get_metrics_by_environment, sink_metrics};
    use chrono::Utc;
    use std::collections::{BTreeMap, HashMap};
    use unleash_edge_types::metrics::{ImpactMetricsKey, MetricsCache};
    use unleash_types::client_metrics::{
        Bucket, BucketMetricSample, ClientMetricsEnv, ImpactMetric, ImpactMetricEnv,
        MetricsMetadata, NumericMetricSample,
    };

    fn create_sample(value: f64, labels: BTreeMap<String, String>) -> NumericMetricSample {
        NumericMetricSample {
            value,
            labels: Some(labels),
        }
    }

    fn create_counter_metric(name: &str, samples: Vec<NumericMetricSample>) -> ImpactMetric {
        ImpactMetric::Counter {
            name: name.into(),
            help: format!("Test counter metric {}", name),
            samples,
        }
    }

    fn create_gauge_metric(name: &str, samples: Vec<NumericMetricSample>) -> ImpactMetric {
        ImpactMetric::Gauge {
            name: name.into(),
            help: format!("Test gauge metric {}", name),
            samples,
        }
    }

    fn create_histogram_metric(name: &str, samples: Vec<BucketMetricSample>) -> ImpactMetric {
        ImpactMetric::Histogram {
            name: name.into(),
            help: format!("Test histogram metric {}", name),
            samples,
        }
    }

    fn create_bucket_sample(count: u64) -> BucketMetricSample {
        BucketMetricSample {
            labels: None,
            count,
            sum: count as f64 * 2.0,
            buckets: vec![
                Bucket {
                    le: 1.0,
                    count: count / 3,
                },
                Bucket {
                    le: 5.0,
                    count: count * 2 / 3,
                },
                Bucket {
                    le: f64::INFINITY,
                    count,
                },
            ],
        }
    }

    fn create_bucket_sample_with_labels(
        count: u64,
        labels: BTreeMap<String, String>,
    ) -> BucketMetricSample {
        BucketMetricSample {
            labels: Some(labels),
            ..create_bucket_sample(count)
        }
    }

    fn create_test_labels(key: &str, value: &str) -> BTreeMap<String, String> {
        BTreeMap::from([(key.into(), value.into())])
    }

    fn no_labels() -> BTreeMap<String, String> {
        BTreeMap::new()
    }

    fn create_and_sink_impact_metrics(
        cache: &MetricsCache,
        app_name: &str,
        env: &str,
        metric_name: &str,
        metric_type: &str,
        value: f64,
    ) {
        let labels = create_test_labels("env", env);
        let metric = match metric_type {
            "counter" => create_counter_metric(metric_name, vec![create_sample(value, labels)]),
            "gauge" => create_gauge_metric(metric_name, vec![create_sample(value, labels)]),
            "histogram" => create_histogram_metric(
                metric_name,
                vec![create_bucket_sample_with_labels(value as u64, labels)],
            ),
            _ => create_counter_metric(metric_name, vec![create_sample(value, labels)]),
        };
        let impact_metrics = vec![ImpactMetricEnv::new(metric, app_name.into(), env.into())];
        sink_impact_metrics(cache, impact_metrics);
    }

    fn create_client_metrics(
        app_name: &str,
        feature_name: &str,
        environment: &str,
        yes: u32,
        no: u32,
    ) -> ClientMetricsEnv {
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

    #[test]
    pub fn counters_aggregate_by_summing() {
        let cache = MetricsCache::default();
        let app = "test_app";
        let env = "test_env";
        let labels = no_labels();
        let counter = vec![create_counter_metric(
            "test_counter",
            vec![
                create_sample(5.0, labels.clone()),
                create_sample(3.0, labels.clone()),
            ],
        )];

        sink_impact_metrics(
            &cache,
            convert_to_impact_metrics_env(counter, app.into(), env.into()),
        );

        let key = ImpactMetricsKey {
            app_name: app.into(),
            environment: env.into(),
        };
        let entry = cache.impact_metrics.get(&key).unwrap();
        let metrics = entry.value();
        match &metrics[0].impact_metric {
            ImpactMetric::Counter { samples, .. } => assert_eq!(samples[0].value, 8.0),
            _ => panic!("Expected counter metric"),
        }
    }

    #[test]
    pub fn gauges_keep_last_value() {
        let cache = MetricsCache::default();
        let app = "test_app";
        let env = "test_env";
        let labels = no_labels();
        let gauge = vec![create_gauge_metric(
            "test_gauge",
            vec![
                create_sample(10.0, labels.clone()),
                create_sample(20.0, labels.clone()),
            ],
        )];

        sink_impact_metrics(
            &cache,
            convert_to_impact_metrics_env(gauge, app.into(), env.into()),
        );

        let key = ImpactMetricsKey {
            app_name: app.into(),
            environment: env.into(),
        };
        let entry = cache.impact_metrics.get(&key).unwrap();
        let metrics = entry.value();
        match &metrics[0].impact_metric {
            ImpactMetric::Gauge { samples, .. } => assert_eq!(samples[0].value, 20.0),
            _ => panic!("Expected gauge metric"),
        }
    }

    #[test]
    pub fn histograms_merge_counts_and_sums() {
        let cache = MetricsCache::default();
        let app = "test_app";
        let env = "test_env";
        let histogram = vec![create_histogram_metric(
            "test_histogram",
            vec![
                create_bucket_sample(3), // count=3, sum=6.0
                create_bucket_sample(7), // count=7, sum=14.0
            ],
        )];

        sink_impact_metrics(
            &cache,
            convert_to_impact_metrics_env(histogram, app.into(), env.into()),
        );

        let key = ImpactMetricsKey {
            app_name: app.into(),
            environment: env.into(),
        };
        let entry = cache.impact_metrics.get(&key).unwrap();
        let metrics = entry.value();
        match &metrics[0].impact_metric {
            ImpactMetric::Histogram { samples, .. } => {
                assert_eq!(samples[0].count, 10);
                assert_eq!(samples[0].sum, 20.0);
                assert_eq!(samples[0].buckets[0].count, 1 + 2); // 3/3 + 7/3
                assert_eq!(samples[0].buckets[1].count, 2 + 4); // 3*2/3 + 7*2/3  
                assert_eq!(samples[0].buckets[2].count, 3 + 7); // total counts
            }
            _ => panic!("Expected histogram metric"),
        }
    }

    #[test]
    pub fn merge_impact_metrics_from_different_apps() {
        let cache = MetricsCache::default();

        let app1_labels = create_test_labels("appName", "app1");
        let app2_labels = create_test_labels("appName", "app2");

        let app1_metrics = vec![create_counter_metric(
            "shared_metric",
            vec![create_sample(5.0, app1_labels)],
        )];
        let app2_metrics = vec![create_counter_metric(
            "shared_metric",
            vec![create_sample(3.0, app2_labels)],
        )];

        cache.impact_metrics.insert(
            ImpactMetricsKey {
                app_name: "app1".into(),
                environment: "test".into(),
            },
            convert_to_impact_metrics_env(app1_metrics, "app1".into(), "test".into()),
        );
        cache.impact_metrics.insert(
            ImpactMetricsKey {
                app_name: "app2".into(),
                environment: "test".into(),
            },
            convert_to_impact_metrics_env(app2_metrics, "app2".into(), "test".into()),
        );

        let all_metrics: Vec<_> = cache
            .impact_metrics
            .iter()
            .flat_map(|e| e.value().clone())
            .collect();
        let merged = merge_impact_metrics(all_metrics);

        assert_eq!(merged.len(), 1);
        match &merged[0].impact_metric {
            ImpactMetric::Counter { samples, .. } => assert_eq!(samples.len(), 2),
            _ => panic!("Expected counter metric"),
        }
    }

    #[test]
    pub fn get_metrics_by_environment_handles_metrics_and_impact_metrics_independently() {
        let cache = MetricsCache::default();
        let app = "test_app";
        let dev_env = "development";
        let prod_env = "production";
        let staging_env = "staging";

        // 1. Development: only regular metrics
        sink_metrics(
            &cache,
            &[create_client_metrics(app, "feature", dev_env, 5, 2)],
        );

        // 2. Production: only impact metrics
        create_and_sink_impact_metrics(&cache, app, prod_env, "counter_metric", "counter", 10.0);

        // 3. Staging: both regular and impact metrics
        sink_metrics(
            &cache,
            &[create_client_metrics(app, "feature", staging_env, 3, 1)],
        );
        create_and_sink_impact_metrics(&cache, app, staging_env, "gauge_metric", "gauge", 42.0);

        let batches = get_metrics_by_environment(&cache);

        assert_eq!(batches[dev_env].metrics.len(), 1);
        assert_eq!(batches[dev_env].impact_metrics.len(), 0);

        assert_eq!(batches[prod_env].metrics.len(), 0);
        assert_eq!(batches[prod_env].impact_metrics.len(), 1);
        assert_eq!(
            batches[prod_env].impact_metrics[0].impact_metric.name(),
            "counter_metric"
        );

        assert_eq!(batches[staging_env].metrics.len(), 1);
        assert_eq!(batches[staging_env].impact_metrics.len(), 1);
        assert_eq!(
            batches[staging_env].impact_metrics[0].impact_metric.name(),
            "gauge_metric"
        );
    }

    #[test]
    pub fn origin_label_is_injected() {
        let cache = MetricsCache::default();
        let app = "test_app";
        let env = "test_env";
        let labels = create_test_labels("app", "my-app");
        let metrics = vec![create_counter_metric(
            "test_metric",
            vec![create_sample(1.0, labels)],
        )];

        sink_impact_metrics(
            &cache,
            convert_to_impact_metrics_env(metrics, app.into(), env.into()),
        );

        let key = ImpactMetricsKey {
            app_name: app.into(),
            environment: env.into(),
        };
        let entry = cache.impact_metrics.get(&key).unwrap();
        let stored_metrics = entry.value();
        let sample_labels = match &stored_metrics[0].impact_metric {
            ImpactMetric::Counter { samples, .. } => &samples[0].labels,
            ImpactMetric::Gauge { samples, .. } => &samples[0].labels,
            ImpactMetric::Histogram { samples, .. } => &samples[0].labels,
        };

        let labels = sample_labels.as_ref().unwrap();
        assert_eq!(labels.get("origin"), Some(&"edge".to_string()));
        assert_eq!(labels.get("app"), Some(&"my-app".to_string()));
    }
}
