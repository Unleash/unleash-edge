use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub struct EnrichmentRequest {
    pub id: u64,
    pub context: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct EnrichmentResponse {
    pub id: u64,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ReadyMessage {
    #[serde(rename = "type")]
    pub message_type: String,
}
