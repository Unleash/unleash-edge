use serde::{Deserialize, Serialize};
use unleash_types::client_metrics::{ClientApplication, ClientMetricsEnv, ImpactMetricEnv};

// This is the number of bytes that are not part of the actual items in the batch.
// It includes the opening and closing brackets, commas between items, and the JSON structure overhead.
// If you patch the MetricsBatch struct, you may need to adjust this value. Prop tests will catch it.
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
