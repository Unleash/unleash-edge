use serde::{Deserialize, Deserializer};
use std::collections::HashMap;
use unleash_types::client_features::Context;

#[expect(dead_code)]
pub(crate) struct EnrichmentRequest {
    pub(crate) id: i64,
    pub(crate) context: Context,
    pub(crate) headers: HashMap<String, String>,
}

pub(crate) struct EnrichmentResponse {
    #[cfg_attr(not(test), expect(dead_code))]
    pub(crate) id: i64,
    #[cfg_attr(not(test), expect(dead_code))]
    pub(crate) outcome: Result<Context, String>,
}

#[derive(Deserialize)]
pub(crate) struct ReadyMessage {
    #[serde(rename = "messageType")]
    pub _message_type: String,
}

#[derive(Deserialize)]
struct ProtocolEnrichmentResponse {
    pub(crate) id: i64,
    pub(crate) context: Option<Context>,
    pub(crate) error: Option<String>,
}

impl<'de> Deserialize<'de> for EnrichmentResponse {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // yeah life sucks here - this is borked on the wire, but importantly this is not
        // a worker telling us about an error state, this is a worker who's unable to communicate with
        // us effectively for some reason - most likely a bug in the protocol itself. Either way, this is
        // a different type of error from a worker reporting a protocol enrichment error
        let protocol_response = ProtocolEnrichmentResponse::deserialize(deserializer)?;

        match (protocol_response.context, protocol_response.error) {
            (Some(context), None) => Ok(EnrichmentResponse {
                id: protocol_response.id,
                outcome: Ok(context),
            }),
            (None, Some(error)) => {
                // errors here are happy path - user defined script is having a moment, but importantly, nothing is wrong with our system
                Ok(EnrichmentResponse {
                    id: protocol_response.id,
                    outcome: Err(error),
                })
            }
            (Some(_), Some(_)) => Err(serde::de::Error::custom(
                "Context enricher protocol error: both context and error are set",
            )),
            (None, None) => Err(serde::de::Error::custom(
                "Context enricher protocol error: response must contain either context or error",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserializing_happy_path_enrichment_response_works() {
        let json = r#"{"id": 1, "context": {"userId": "123"}, "error": null}"#;
        let response: EnrichmentResponse = serde_json::from_str(json)
            .expect("Failed to deserialize happy path enrichment response");

        assert_eq!(response.id, 1);
        assert!(response.outcome.is_ok());
        assert_eq!(response.outcome.unwrap().user_id.as_deref(), Some("123"));
    }

    #[test]
    fn test_deserializing_error_path_enrichment_response_works() {
        let json = r#"{"id": 2, "context": null, "error": "Some error occurred"}"#;
        let response: EnrichmentResponse = serde_json::from_str(json)
            .expect("Failed to deserialize error path enrichment response");

        assert_eq!(response.id, 2);
        assert!(response.outcome.is_err());
        assert_eq!(response.outcome.unwrap_err(), "Some error occurred");
    }

    #[test]
    fn broken_protocol_message_errors() {
        let json = r#"{"id": 3, "context": {"userId": "123"}, "error": "Some error occurred"}"#;
        let both_fields_set_response: Result<EnrichmentResponse, _> = serde_json::from_str(json);

        let json = r#"{"id": 4, "context": null, "error": null}"#;
        let neither_field_set_response: Result<EnrichmentResponse, _> = serde_json::from_str(json);

        assert!(both_fields_set_response.is_err());
        assert!(neither_field_set_response.is_err());
    }
}
