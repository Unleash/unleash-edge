use std::io::{self, Write};
use tracing::instrument;
use unleash_edge_types::metrics::batching::MetricsBatch;
use unleash_types::client_metrics::{ClientApplication, ClientMetricsEnv, ImpactMetricEnv};

pub const UPSTREAM_MAX_BODY_SIZE: usize = 100 * 1024;
pub const BATCH_BODY_SIZE: usize = 95 * 1024;

pub(crate) fn sendable(batch: &MetricsBatch) -> bool {
    size_of_batch(batch) < UPSTREAM_MAX_BODY_SIZE
}

pub(crate) fn size_of_batch<T: serde::Serialize>(value: &T) -> usize {
    let mut counter = ByteCounter { count: 0 };
    serde_json::to_writer(&mut counter, &value).unwrap();
    counter.count
}

// This is the number of bytes that are not part of the actual items in the batch.
// It includes the opening and closing brackets, commas between items, and the JSON structure overhead.
// If you patch the MetricsBatch struct, you may need to adjust this value. Prop tests will catch it.
const EMPTY_BATCH_JSON_OVERHEAD: usize = 51;

struct SizedItem {
    size: usize,
    item: SizedItemKind,
}

enum SizedItemKind {
    App(ClientApplication),
    Metric(ClientMetricsEnv),
    Impact(ImpactMetricEnv),
}

impl SizedItemKind {
    pub fn serialized_len(&self) -> usize {
        match self {
            SizedItemKind::App(app) => size_of_batch(app),
            SizedItemKind::Metric(metric) => size_of_batch(metric),
            SizedItemKind::Impact(impact) => size_of_batch(impact),
        }
    }
}

impl SizedItem {
    pub fn new(item: SizedItemKind) -> SizedItem {
        SizedItem {
            size: item.serialized_len(),
            item,
        }
    }
}
struct ByteCounter {
    count: usize,
}

impl Write for ByteCounter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.count += buf.len();
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn partition_batch(mut batch: MetricsBatch, max_batch_size: usize) -> Vec<MetricsBatch> {
    let mut sized_items = Vec::new();

    // applications are necessarily first, since Unleash needs to know the application in
    // order to store the metrics. Impact metrics are last by coincidence since they're independent
    sized_items.extend(
        batch
            .applications
            .drain(..)
            .map(|app| SizedItem::new(SizedItemKind::App(app))),
    );
    sized_items.extend(
        batch
            .metrics
            .drain(..)
            .map(|metric| SizedItem::new(SizedItemKind::Metric(metric))),
    );
    sized_items.extend(
        batch
            .impact_metrics
            .drain(..)
            .map(|impact| SizedItem::new(SizedItemKind::Impact(impact))),
    );

    let mut batches = Vec::new();
    let mut current_batch = MetricsBatch::default();
    let mut current_size = EMPTY_BATCH_JSON_OVERHEAD;

    for item in sized_items {
        let next_size = current_size + item.size + 1; // for commas separating items

        if next_size > max_batch_size {
            if !current_batch.applications.is_empty()
                || !current_batch.metrics.is_empty()
                || !current_batch.impact_metrics.is_empty()
            {
                batches.push(current_batch);
            }
            current_batch = MetricsBatch::default();
            current_size = EMPTY_BATCH_JSON_OVERHEAD;
        }

        match item.item {
            SizedItemKind::App(app) => current_batch.applications.push(app),
            SizedItemKind::Metric(metric) => current_batch.metrics.push(metric),
            SizedItemKind::Impact(impact) => current_batch.impact_metrics.push(impact),
        }

        current_size += item.size + 1; // for commas separating items
    }

    if !current_batch.applications.is_empty()
        || !current_batch.metrics.is_empty()
        || !current_batch.impact_metrics.is_empty()
    {
        batches.push(current_batch);
    }

    batches
}

#[instrument(skip(batch))]
pub(crate) fn cut_into_sendable_batches(batch: MetricsBatch) -> Vec<MetricsBatch> {
    partition_batch(batch, BATCH_BODY_SIZE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client_metrics::{get_appropriately_sized_batches, sink_metrics};
    use chrono::{DateTime, Utc};
    use proptest::prelude::*;
    use std::collections::HashMap;
    use test_case::test_case;
    use unleash_edge_types::metrics::{ApplicationKey, MetricsCache};
    use unleash_types::client_metrics::SdkType::Backend;
    use unleash_types::client_metrics::{
        ClientApplication, ClientMetricsEnv, ConnectVia, ImpactMetric, MetricsMetadata,
    };

    fn make_client_app<T>(app_id: T) -> ClientApplication
    where
        T: std::fmt::Display,
    {
        ClientApplication {
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
        }
    }

    fn make_metrics_env<T>(toggle_id: T) -> ClientMetricsEnv
    where
        T: std::fmt::Display,
    {
        ClientMetricsEnv {
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
        }
    }

    fn make_impact_metric_env<T>(toggle_id: T) -> ImpactMetricEnv
    where
        T: std::fmt::Display,
    {
        ImpactMetricEnv {
            app_name: format!("app_name_{}", toggle_id),
            environment: "development".into(),
            impact_metric: ImpactMetric {
                name: format!("impact-{}", toggle_id),
                help: "Impact metric for testing".into(),
                r#type: "counter".into(),
                samples: vec![],
            },
        }
    }

    #[test_case(10, 100, 1; "10 apps 100 toggles. Will not be split")]
    #[test_case(1, 10000, 26; "1 app 10k toggles, will be split into 26 batches")]
    #[test_case(1000, 1000, 7; "1000 apps 1000 toggles, will be split into 7 batches")]
    #[test_case(500, 5000, 15; "500 apps 5000 toggles, will be split into 15 batches")]
    #[test_case(5000, 1, 20; "5000 apps 1 metric will be split")]
    fn splits_successfully_into_sendable_chunks(apps: u64, toggles: u64, batch_count: usize) {
        let apps: Vec<ClientApplication> = (1..=apps).map(make_client_app).collect();

        let toggles: Vec<ClientMetricsEnv> = (1..=toggles).map(make_metrics_env).collect();

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
        sink_metrics(&cache, &toggles);
        let batches = get_appropriately_sized_batches(&cache);

        assert_eq!(batches.len(), batch_count);
        assert!(batches.iter().all(sendable));
        // Check that we have no duplicates
        let applications_sent_count = batches.iter().flat_map(|b| b.applications.clone()).count();

        assert_eq!(applications_sent_count, apps.len());

        let metrics_sent_count = batches.iter().flat_map(|b| b.metrics.clone()).count();
        assert_eq!(metrics_sent_count, toggles.len());
    }

    fn execute_test(
        apps: Vec<String>,
        metrics: Vec<String>,
        impacts: Vec<String>,
        batch_size: usize,
    ) {
        let apps = apps.into_iter().map(make_client_app).collect();
        let metrics = metrics.into_iter().map(make_metrics_env).collect();
        let impacts = impacts.into_iter().map(make_impact_metric_env).collect();

        let batch = MetricsBatch {
            applications: apps,
            metrics,
            impact_metrics: impacts,
        };

        let total_apps = batch.applications.len();
        let total_metrics = batch.metrics.len();
        let total_impacts = batch.impact_metrics.len();

        let batches = partition_batch(batch, batch_size);

        // Invariants
        let output_apps = batches.iter().map(|b| b.applications.len()).sum::<usize>();
        let output_metrics = batches.iter().map(|b| b.metrics.len()).sum::<usize>();
        let output_impacts = batches
            .iter()
            .map(|b| b.impact_metrics.len())
            .sum::<usize>();

        assert_eq!(total_apps, output_apps);
        assert_eq!(total_metrics, output_metrics);
        assert_eq!(total_impacts, output_impacts);

        for b in batches {
            let mut counter = ByteCounter { count: 0 };
            serde_json::to_writer(&mut counter, &b).unwrap();
            let json_size = counter.count;
            println!("Batch size: {}, JSON size: {}", batch_size, json_size);
            println!("Batch as bytes:");
            println!("{:02X?}", &b);

            assert!(json_size <= batch_size);
        }
    }

    proptest! {
        #[test]
        fn prop_batches_obey_size_and_correctness(
            apps in proptest::collection::vec(any::<String>(), 1..100),
            metrics in proptest::collection::vec(any::<String>(), 1..100),
            impacts in proptest::collection::vec(any::<String>(), 1..100)
        ) {
            execute_test(apps, metrics, impacts, 2000);
        }
    }

    #[test]
    fn invalid_when_container_size_is_2_not_51() {
        let apps = vec![""].into_iter().map(|x| x.to_string()).collect();
        let metrics = vec!["", ""].into_iter().map(|x| x.to_string()).collect();
        let impacts = vec![
            " 豈豈𑃐𑥐a࿎Σ\\AAﹰ Σ a0🌀ⴧa ጘ𐇐®AAaΣ ",
            "ຄ𛄲Όຄ𑊊؆ꬠ\"a￼ዂAA ᤰ🠀 aaᝠAA𝈀ﷰa a𝈀 ",
            "AAaA 🌀\\AA A 🌀AA\"aቘ 🈐🌀🌀  \"00a ಎ",
            "𐐀  ﹨𑌓 a𞹴 00🠀 aA AA¡  \u{d00}A¡",
        ]
        .into_iter()
        .map(|x| x.to_string())
        .collect();
        execute_test(apps, metrics, impacts, 600);
    }
}
