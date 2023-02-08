use std::collections::HashMap;
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

#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub struct MetricsKey {
    pub app_name: String,
    pub feature_name: String,
}

pub struct MetricsBatch {
    pub applications: Vec<ClientApplication>,
    pub metrics: Vec<ClientMetricsEnv>,
}

#[derive(Default)]
pub struct MetricsCache {
    pub applications: HashMap<ApplicationKey, ClientApplication>,
    pub metrics: HashMap<MetricsKey, ClientMetricsEnv>,
}

impl MetricsCache {
    pub fn get_unsent_metrics(&self) -> MetricsBatch {
        MetricsBatch {
            applications: self.applications.values().cloned().collect(),
            metrics: self.metrics.values().cloned().collect(),
        }
    }
    pub fn reset_metrics(&mut self) {
        self.applications.clear();
        self.metrics.clear();
    }
}
