use serde::{Deserialize, Serialize};
use unleash_types::client_metrics::{ClientApplication, ClientMetricsEnv, ImpactMetricEnv};

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
    #[serde(skip)]
    pub seen_tokens: Vec<String>,
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        rename = "seenTokenHashes"
    )]
    pub seen_token_hashes: Vec<String>,
}
