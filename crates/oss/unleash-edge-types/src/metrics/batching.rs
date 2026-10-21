use serde::{Deserialize, Serialize};
use unleash_types::client_metrics::{ClientApplication, ClientMetricsEnv, ImpactMetricEnv};

pub const EMPTY_BATCH_JSON_OVERHEAD: usize = 67;

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
    #[serde(default, skip_serializing_if = "Vec::is_empty", rename = "seenTokens")]
    pub seen_tokens: Vec<String>,
}
