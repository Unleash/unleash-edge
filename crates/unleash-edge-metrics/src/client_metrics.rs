use crate::client_impact_metrics::{
    convert_to_impact_metrics_env, merge_impact_metrics, sink_impact_metrics,
};
use crate::metric_batching::{cut_into_sendable_batches, sendable, size_of_batch};
use itertools::Itertools;
use lazy_static::lazy_static;
use prometheus::{Histogram, IntCounterVec, register_histogram, register_int_counter_vec};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::debug;
use unleash_edge_types::metrics::batching::MetricsBatch;
use unleash_edge_types::metrics::{ApplicationKey, ImpactMetricsKey, MetricsCache};
use unleash_edge_types::tokens::EdgeToken;
use unleash_edge_types::{BatchMetricsRequestBody, MetricsKey};
use unleash_types::client_metrics::SdkType::Backend;
use unleash_types::client_metrics::{
    ClientApplication, ClientMetrics, ClientMetricsEnv, ConnectVia, MetricsMetadata,
};

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

pub fn get_metrics_by_environment(cache: &MetricsCache) -> HashMap<String, MetricsBatch> {
    let mut batches_by_environment = HashMap::new();

    let applications = cache
        .applications
        .iter()
        .map(|e| e.value().clone())
        .collect::<Vec<ClientApplication>>();

    let mut all_environments = std::collections::HashSet::new();

    for entry in cache.metrics.iter() {
        all_environments.insert(entry.value().environment.clone());
    }

    for entry in cache.impact_metrics.iter() {
        all_environments.insert(entry.key().environment.clone());
    }

    let data = cache
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
        for entry in cache.impact_metrics.iter() {
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

pub fn get_appropriately_sized_env_batches(
    cache: &MetricsCache,
    batch: &MetricsBatch,
) -> Vec<MetricsBatch> {
    for app in batch.applications.clone() {
        cache
            .applications
            .remove(&ApplicationKey::from(app.clone()));
    }

    for impact_metric in batch.impact_metrics.clone() {
        cache
            .impact_metrics
            .remove(&ImpactMetricsKey::from(&impact_metric));
    }

    for metric in batch.metrics.clone() {
        cache.metrics.remove(&MetricsKey::from(metric.clone()));
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
pub fn get_appropriately_sized_batches(cache: &MetricsCache) -> Vec<MetricsBatch> {
    let impact_keys: Vec<ImpactMetricsKey> = cache
        .impact_metrics
        .iter()
        .map(|e| e.key().clone())
        .collect();

    let mut all_impact_metrics = Vec::new();
    for entry in cache.impact_metrics.iter() {
        all_impact_metrics.extend(entry.value().clone());
    }

    let merged_impact_metrics = merge_impact_metrics(all_impact_metrics);

    let batch = MetricsBatch {
        applications: cache
            .applications
            .iter()
            .map(|e| e.value().clone())
            .collect(),
        metrics: cache
            .metrics
            .iter()
            .map(|e| e.value().clone())
            .filter(|m| m.yes > 0 || m.no > 0) // Makes sure that we only return buckets that have values. We should have a test for this :P
            .collect(),
        impact_metrics: merged_impact_metrics,
    };
    for app in batch.applications.clone() {
        cache
            .applications
            .remove(&ApplicationKey::from(app.clone()));
    }

    for key in &impact_keys {
        cache.impact_metrics.remove(key);
    }

    for metric in batch.metrics.clone() {
        cache.metrics.remove(&MetricsKey::from(metric.clone()));
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

pub fn reinsert_batch(cache: &MetricsCache, batch: MetricsBatch) {
    for application in batch.applications {
        register_application(cache, application);
    }

    sink_impact_metrics(cache, batch.impact_metrics.clone());

    sink_metrics(cache, &batch.metrics);
}

pub fn sink_bulk_metrics(
    cache: &MetricsCache,
    metrics: BatchMetricsRequestBody,
    connect_via: &ConnectVia,
) {
    for application in metrics.applications {
        register_application(
            cache,
            application.connect_via(&connect_via.app_name, &connect_via.instance_id),
        )
    }

    // TODO: sink impact metrics

    sink_metrics(cache, &metrics.metrics)
}

pub fn reset_metrics(cache: &MetricsCache) {
    cache.applications.clear();
    cache.metrics.clear();
    cache.impact_metrics.clear();
}

pub fn register_application(cache: &MetricsCache, application: ClientApplication) {
    cache
        .applications
        .insert(ApplicationKey::from(application.clone()), application);
}

pub fn sink_metrics(cache: &MetricsCache, metrics: &[ClientMetricsEnv]) {
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
        cache
            .metrics
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

pub fn register_client_application(
    edge_token: EdgeToken,
    connect_via: &ConnectVia,
    client_application: ClientApplication,
    metrics_cache: Arc<MetricsCache>,
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

pub fn register_client_metrics(
    edge_token: EdgeToken,
    metrics: ClientMetrics,
    metrics_cache: Arc<MetricsCache>,
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
        sink_impact_metrics(&metrics_cache, impact_metrics_env);
    }

    sink_metrics(&metrics_cache, &client_metrics_env);
}

pub fn register_bulk_metrics(
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
    sink_bulk_metrics(metrics_cache, updated, connect_via);
}
