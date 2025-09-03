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
        .map(|metric| (metric.impact_metric.name.clone(), metric))
        .collect()
}

fn reduce_metrics_samples(metric: ImpactMetricEnv) -> ImpactMetricEnv {
    let mut empty_metric = ImpactMetricEnv::new(
        ImpactMetric {
            name: metric.impact_metric.name.clone(),
            help: metric.impact_metric.help.clone(),
            r#type: metric.impact_metric.r#type.clone(),
            samples: vec![],
        },
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
        let metric_name = metric.impact_metric.name.clone();

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
                aggregated_metrics.get_mut(&reduced_metric.impact_metric.name)
            {
                existing_metric.merge(reduced_metric);
            } else {
                aggregated_metrics
                    .insert(reduced_metric.impact_metric.name.clone(), reduced_metric);
            }
        }
        let layered_metrics = aggregated_metrics
            .into_values()
            .map(|mut metric| {
                for sample in &mut metric.impact_metric.samples {
                    if let Some(labels) = &mut sample.labels {
                        labels.insert("origin".to_string(), "edge".into());
                    } else {
                        sample.labels = Some(BTreeMap::from([("origin".into(), "edge".into())]));
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
        ClientMetricsEnv, ImpactMetric, ImpactMetricEnv, MetricsMetadata,
    };

    fn create_sample(
        value: f64,
        labels: BTreeMap<String, String>,
    ) -> unleash_types::client_metrics::MetricSample {
        unleash_types::client_metrics::MetricSample {
            value,
            labels: Some(labels),
        }
    }

    fn create_impact_metric(
        name: &str,
        r#type: &str,
        samples: Vec<unleash_types::client_metrics::MetricSample>,
    ) -> ImpactMetric {
        ImpactMetric {
            name: name.into(),
            help: format!("Test {} metric", r#type),
            r#type: r#type.into(),
            samples,
        }
    }

    fn create_test_labels(key: &str, value: &str) -> BTreeMap<String, String> {
        BTreeMap::from([(key.into(), value.into())])
    }

    fn create_and_sink_impact_metrics(
        cache: &MetricsCache,
        app_name: &str,
        env: &str,
        metric_name: &str,
        metric_type: &str,
        value: f64,
    ) {
        let labels = BTreeMap::from([("env".into(), env.into())]);
        let impact_metrics = vec![ImpactMetricEnv::new(
            create_impact_metric(metric_name, metric_type, vec![create_sample(value, labels)]),
            app_name.into(),
            env.into(),
        )];
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
    pub fn sink_impact_metrics_aggregates_correctly() {
        // Setup
        let cache = MetricsCache::default();
        let app = "test_app";
        let env = "test_env";
        let test_key = ImpactMetricsKey {
            app_name: app.into(),
            environment: env.into(),
        };

        let labels1 = create_test_labels("label1", "value1");
        let labels2 = create_test_labels("label1", "different");

        let counter_metrics = vec![create_impact_metric(
            "test_counter",
            "counter",
            vec![
                create_sample(1.0, labels1.clone()),
                create_sample(2.0, labels1.clone()),
                create_sample(3.0, labels2.clone()),
            ],
        )];

        let gauge_metrics = vec![create_impact_metric(
            "test_gauge",
            "gauge",
            vec![
                create_sample(1.0, labels1.clone()),
                create_sample(2.0, labels1.clone()),
            ],
        )];

        sink_impact_metrics(
            &cache,
            convert_to_impact_metrics_env(counter_metrics, app.into(), env.into()),
        );
        sink_impact_metrics(
            &cache,
            convert_to_impact_metrics_env(gauge_metrics, app.into(), env.into()),
        );

        let aggregated_metrics = cache.impact_metrics.get(&test_key).unwrap();
        let counter = aggregated_metrics
            .value()
            .iter()
            .find(|m| m.impact_metric.name == "test_counter")
            .unwrap();

        let value1_sample = counter
            .impact_metric
            .samples
            .iter()
            .find(|s| s.labels.as_ref().unwrap().get("label1") == Some(&"value1".into()))
            .unwrap();
        assert_eq!(value1_sample.value, 3.0, "Counter values should be summed");

        let gauge = aggregated_metrics
            .value()
            .iter()
            .find(|m| m.impact_metric.name == "test_gauge")
            .unwrap();
        assert_eq!(
            gauge.impact_metric.samples[0].value, 2.0,
            "Gauge should have the last value"
        );
    }

    #[test]
    pub fn merge_impact_metrics_from_different_apps() {
        let cache = MetricsCache::default();
        let env = "default";

        let app1 = "app1";
        let app1_key = ImpactMetricsKey {
            app_name: app1.into(),
            environment: env.into(),
        };
        let app1_labels = BTreeMap::from([("appName".into(), "my-application-1".into())]);
        let app1_metrics = vec![create_impact_metric(
            "test",
            "counter",
            vec![create_sample(10.0, app1_labels)],
        )];

        let app2 = "app2";
        let app2_key = ImpactMetricsKey {
            app_name: app2.into(),
            environment: env.into(),
        };
        let app2_labels = BTreeMap::from([("appName".into(), "my-application-2".into())]);
        let app2_metrics = vec![create_impact_metric(
            "test",
            "counter",
            vec![create_sample(1.0, app2_labels)],
        )];

        cache.impact_metrics.insert(
            app1_key,
            convert_to_impact_metrics_env(app1_metrics, app1.into(), env.into()),
        );
        cache.impact_metrics.insert(
            app2_key,
            convert_to_impact_metrics_env(app2_metrics, app2.into(), env.into()),
        );

        let mut all_impact_metrics = Vec::new();
        for entry in cache.impact_metrics.iter() {
            all_impact_metrics.extend(entry.value().clone());
        }
        let merged_impact_metrics = merge_impact_metrics(all_impact_metrics);

        assert_eq!(
            merged_impact_metrics.len(),
            1,
            "Should have one merged metric"
        );
        let test_metric = &merged_impact_metrics[0];

        let app1_value = test_metric
            .impact_metric
            .samples
            .iter()
            .find(|s| s.labels.as_ref().unwrap().get("appName") == Some(&"my-application-1".into()))
            .unwrap()
            .value;
        let app2_value = test_metric
            .impact_metric
            .samples
            .iter()
            .find(|s| s.labels.as_ref().unwrap().get("appName") == Some(&"my-application-2".into()))
            .unwrap()
            .value;

        assert_eq!(app1_value, 10.0, "App1 sample value should be preserved");
        assert_eq!(app2_value, 1.0, "App2 sample value should be preserved");
    }

    #[test]
    pub fn get_metrics_by_environment_handles_metrics_and_impact_metrics_independently() {
        let cache = MetricsCache::default();

        // 1. Development: only regular metrics
        let dev_env = "development";
        let dev_app = "dev-app";
        sink_metrics(
            &cache,
            &[create_client_metrics(dev_app, "feature", dev_env, 5, 2)],
        );

        // 2. Production: only impact metrics
        let prod_env = "production";
        let prod_app = "prod-app";
        create_and_sink_impact_metrics(&cache, prod_app, prod_env, "counter", "counter", 10.0);

        // 3. Staging: both regular and impact metrics
        let staging_env = "staging";
        let staging_app = "staging-app";
        sink_metrics(
            &cache,
            &[create_client_metrics(
                staging_app,
                "feature",
                staging_env,
                3,
                1,
            )],
        );
        create_and_sink_impact_metrics(&cache, staging_app, staging_env, "gauge", "gauge", 42.0);

        let batches = get_metrics_by_environment(&cache);

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

    #[test]
    pub fn sink_impact_metrics_injects_origin_label() {
        let cache = MetricsCache::default();
        let app = "test_app";
        let env = "test_env";

        let test_key = ImpactMetricsKey {
            app_name: app.into(),
            environment: env.into(),
        };

        let labels = create_test_labels("some_label", "some_value");

        let metrics = vec![create_impact_metric(
            "proxy_metric",
            "counter",
            vec![create_sample(5.0, labels.clone())],
        )];

        sink_impact_metrics(
            &cache,
            convert_to_impact_metrics_env(metrics, app.into(), env.into()),
        );

        let stored_metrics = cache.impact_metrics.get(&test_key).unwrap();
        let metric = stored_metrics
            .value()
            .iter()
            .find(|m| m.impact_metric.name == "proxy_metric")
            .unwrap();

        let sample = &metric.impact_metric.samples[0];
        let sample_labels = sample.labels.as_ref().expect("Sample should have labels");

        assert_eq!(
            sample_labels.get("origin"),
            Some(&"edge".to_string()),
            "Should inject 'origin' label with edge value"
        );

        // double check original labels are preserved
        assert_eq!(
            sample_labels.get("some_label"),
            Some(&"some_value".to_string()),
            "Original labels should be preserved"
        );
    }
}
