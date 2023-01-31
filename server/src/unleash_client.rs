use std::str::FromStr;
use std::time::Duration;

use actix_web::http::{
    header::{ContentType, EntityTag, IfNoneMatch},
    StatusCode,
};
use awc::{Client, ClientRequest};
use ulid::Ulid;
use unleash_types::{
    client_features::ClientFeatures,
    client_metrics::{ClientApplication, ClientMetrics},
};
use url::Url;

use crate::types::{
    ClientFeaturesResponse, EdgeResult, RegisterClientApplicationRequest,
    RegisterClientMetricsRequest,
};
use crate::urls::UnleashUrls;
use crate::{error::EdgeError, types::ClientFeaturesRequest};

const UNLEASH_APPNAME_HEADER: &str = "UNLEASH-APPNAME";
const UNLEASH_INSTANCE_ID_HEADER: &str = "UNLEASH-INSTANCEID";
const UNLEASH_CLIENT_SPEC_HEADER: &str = "Unleash-Client-Spec";
const USER_AGENT_HEADER: &str = "User-Agent";

#[derive(Clone)]
pub struct UnleashClient {
    pub urls: UnleashUrls,
    backing_client: Client,
}

pub fn new_awc_client(instance_id: String) -> Client {
    Client::builder()
        .add_default_header((UNLEASH_APPNAME_HEADER, "unleash-edge"))
        .add_default_header((UNLEASH_INSTANCE_ID_HEADER, instance_id))
        .add_default_header(
            (UNLEASH_CLIENT_SPEC_HEADER, "4.2.2"), // yggdrasil::CLIENT_SPEC_VERSION).into(),
        )
        .add_default_header((USER_AGENT_HEADER, "unleash_edge"))
        .add_default_header(ContentType::json())
        .timeout(Duration::from_secs(5))
        .finish()
}

impl UnleashClient {
    pub fn from_url(server_url: Url) -> Self {
        Self {
            urls: UnleashUrls::from_base_url(server_url),
            backing_client: new_awc_client(Ulid::new().to_string()),
        }
    }
    pub fn new(server_url: &str, instance_id_opt: Option<String>) -> Result<Self, EdgeError> {
        let instance_id = instance_id_opt.unwrap_or(Ulid::new().to_string());
        Ok(Self {
            urls: UnleashUrls::from_str(server_url)?,
            backing_client: new_awc_client(instance_id),
        })
    }

    fn awc_client_features_req(&self, req: ClientFeaturesRequest) -> ClientRequest {
        let client_req = self
            .backing_client
            .get(self.urls.client_features_url.to_string())
            .insert_header(("Authorization", req.api_key));
        if let Some(tag) = req.etag {
            client_req.insert_header(IfNoneMatch::Items(vec![tag]))
        } else {
            client_req
        }
    }

    pub async fn get_client_features(
        &self,
        request: ClientFeaturesRequest,
    ) -> EdgeResult<ClientFeaturesResponse> {
        let mut result = self
            .awc_client_features_req(request.clone())
            .send()
            .await
            .map_err(|_| EdgeError::ClientFeaturesFetchError)?;
        if result.status() == StatusCode::NOT_MODIFIED {
            Ok(ClientFeaturesResponse {
                features: None,
                etag: request.etag,
            })
        } else {
            Ok(ClientFeaturesResponse {
                features: result
                    .json::<ClientFeatures>()
                    .await
                    .map(Some)
                    .map_err(|payload_error| EdgeError::ClientFeaturesParseError(payload_error))?,
                etag: result
                    .headers()
                    .get("ETag")
                    .and_then(|etag| EntityTag::from_str(etag.to_str().unwrap().into()).ok()),
            })
        }
    }
}

#[cfg(test)]
#[actix_web::test]
async fn client_can_get_features() {
    let api_key = "*:development.a113e11e04133c367f5fa7c731f9293c492322cf9d6060812cfe3fea";
    let client = UnleashClient::new("https://app.unleash-hosted.com/demo", None).unwrap();
    let client_features_result = client
        .get_client_features(ClientFeaturesRequest::new(api_key.to_string(), None))
        .await;
    assert!(client_features_result.is_ok());
    let client_features_response = client_features_result.unwrap();
    assert!(client_features_response.features.is_some());
    assert!(!client_features_response
        .features
        .unwrap()
        .features
        .is_empty());
    assert!(client_features_response.etag.is_some());
}

#[test]
pub fn can_parse_entity_tag() {
    let etag = EntityTag::from_str("W/\"b5e6-DPC/1RShRw1J/jtxvRtTo1jf4+o\"".into()).unwrap();
    assert!(etag.weak);
}
