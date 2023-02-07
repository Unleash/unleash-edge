use std::collections::HashMap;
use unleash_types::client_metrics::{ClientApplication, MetricBucket};

#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub struct MetricsKey {
    pub app_name: String,
    pub instance_id: String,
}

impl MetricsKey {
    pub fn from_app_name(app_name: String) -> Self {
        Self {
            app_name,
            instance_id: ulid::Ulid::new().to_string(),
        }
    }
}

pub struct MetricsBatch {}

#[derive(Default)]
pub struct MetricsCache {
    pub applications: HashMap<MetricsKey, ClientApplication>,
    pub metrics: HashMap<MetricsKey, MetricBucket>,
}

impl MetricsCache {
    pub fn get_unsent_metrics(&self) -> MetricsBatch {
        MetricsBatch {}
    }
    pub fn reset_metrics(&mut self) {}
}
