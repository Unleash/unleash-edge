use actix_web::http::header::{ContentType, EntityTag, IfNoneMatch};
use actix_web::http::StatusCode;
use awc::{Client, ClientRequest};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::time::Duration;
use ulid::Ulid;
use unleash_types::client_features::ClientFeatures;
use url::Url;

use crate::types::{
    ClientFeaturesResponse, EdgeResult, EdgeToken, TokenStatus, ValidateTokenRequest,
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
        .add_default_header((
            UNLEASH_CLIENT_SPEC_HEADER,
            unleash_yggdrasil::SUPPORTED_SPEC_VERSION,
        ))
        .add_default_header((USER_AGENT_HEADER, "unleash_edge"))
        .add_default_header(ContentType::json())
        .timeout(Duration::from_secs(5))
        .finish()
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EdgeTokens {
    pub tokens: Vec<EdgeToken>,
}

impl UnleashClient {
    pub fn from_url(server_url: Url) -> Self {
        Self {
            urls: UnleashUrls::from_base_url(server_url),
            backing_client: new_awc_client(Ulid::new().to_string()),
        }
    }
    pub fn new(server_url: &str, instance_id_opt: Option<String>) -> Result<Self, EdgeError> {
        let instance_id = instance_id_opt.unwrap_or_else(|| Ulid::new().to_string());
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
    fn awc_validate_token_req(&self) -> ClientRequest {
        self.backing_client
            .post(self.urls.edge_validate_url.to_string())
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
            Ok(ClientFeaturesResponse::NoUpdate(
                request.etag.expect("Got NOT_MODIFIED without an ETag"),
            ))
        } else {
            let features = result
                .json::<ClientFeatures>()
                .await
                .map_err(EdgeError::ClientFeaturesParseError)?;
            let etag = result
                .headers()
                .get("ETag")
                .and_then(|etag| EntityTag::from_str(etag.to_str().unwrap()).ok());
            Ok(ClientFeaturesResponse::Updated(features, etag))
        }
    }
    pub async fn validate_secret(&self, request: ValidateTokenRequest) -> EdgeResult<TokenStatus> {
        let mut result = self
            .awc_validate_token_req()
            .send_body(serde_json::to_string(&request.validation_request).unwrap())
            .await
            .map_err(|_| EdgeError::EdgeTokenError)?;
        match result.status() {
            StatusCode::FORBIDDEN => Ok(TokenStatus::Invalid),
            StatusCode::OK => {
                println!("Had an ok response");
                let token_response = result
                    .json::<EdgeTokens>()
                    .await
                    .map_err(|_| EdgeError::EdgeTokenParseError)?;
                match token_response.tokens.len() {
                    0 => Ok(TokenStatus::Invalid),
                    _ => {
                        let validated_token = token_response.tokens.get(0).unwrap();
                        Ok(TokenStatus::Valid(validated_token.clone()))
                    }
                }
            }
            _ => {
                println!("{}", result.status());
                Err(EdgeError::EdgeTokenError)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        types::{
            ClientFeaturesRequest, ClientFeaturesResponse, TokenStatus, ValidateTokenRequest,
            ValidationRequest,
        },
        unleash_client::UnleashClient,
    };
    use actix_http::HttpService;
    use actix_http_test::{test_server, TestServer};
    use actix_middleware_etag::Etag;
    use actix_service::map_config;
    use actix_web::{dev::AppConfig, http::header::EntityTag, web, App, HttpResponse};
    use std::str::FromStr;
    use unleash_types::client_features::{ClientFeature, ClientFeatures};
    fn two_client_features() -> ClientFeatures {
        ClientFeatures {
            version: 2,
            features: vec![
                ClientFeature {
                    name: "test1".into(),
                    feature_type: Some("release".into()),
                    ..Default::default()
                },
                ClientFeature {
                    name: "test2".into(),
                    feature_type: Some("release".into()),
                    ..Default::default()
                },
            ],
            segments: None,
            query: None,
        }
    }
    async fn return_client_features() -> HttpResponse {
        HttpResponse::Ok().json(two_client_features())
    }

    async fn test_features_server() -> TestServer {
        test_server(move || {
            HttpService::new(map_config(
                App::new().wrap(Etag::default()).service(
                    web::resource("/api/client/features")
                        .route(web::get().to(return_client_features)),
                ),
                |_| AppConfig::default(),
            ))
            .tcp()
        })
        .await
    }

    fn expected_etag(features: ClientFeatures) -> String {
        let hash = features.xx3_hash().unwrap();
        let len = serde_json::to_string(&features)
            .map(|string| string.as_bytes().len())
            .unwrap();
        format!("{len:x}-{hash}")
    }
    #[actix_web::test]
    async fn client_can_get_features() {
        let srv = test_features_server().await;
        let tag = EntityTag::new_weak(expected_etag(two_client_features()));

        let client = UnleashClient::new(srv.url("/").as_str(), None).unwrap();
        let client_features_result = client
            .get_client_features(ClientFeaturesRequest::new("somekey".to_string(), None))
            .await;
        assert!(client_features_result.is_ok());
        let client_features_response = client_features_result.unwrap();
        match client_features_response {
            ClientFeaturesResponse::Updated(f, e) => {
                assert!(e.is_some());
                assert_eq!(e.unwrap(), tag);
                assert!(!f.features.is_empty());
            }
            _ => panic!("Got no update when expecting an update"),
        }
    }

    #[actix_web::test]
    async fn client_handles_304() {
        let api_key = "*:development.a113e11e04133c367f5fa7c731f9293c492322cf9d6060812cfe3fea";
        let srv = test_features_server().await;
        let tag = expected_etag(two_client_features());
        let client = UnleashClient::new(srv.url("/").as_str(), None).unwrap();
        let client_features_result = client
            .get_client_features(ClientFeaturesRequest::new(
                api_key.to_string(),
                Some(tag.clone()),
            ))
            .await;
        assert!(client_features_result.is_ok());
        let client_features_response = client_features_result.unwrap();
        match client_features_response {
            ClientFeaturesResponse::NoUpdate(t) => {
                assert_eq!(t, EntityTag::new_weak(tag));
            }
            _ => panic!("Got an update when no update was expected"),
        }
    }

    #[actix_web::test]
    async fn can_validate_secret() {
        let api_key = "[]:development.08bce4267a3b1aa";
        let client = UnleashClient::new("https://app.unleash-hosted.com/hosted", None)
            .expect("Couldn't create client");
        let validate_result = client
            .validate_secret(ValidateTokenRequest {
                api_key: api_key.to_string(),
                validation_request: ValidationRequest {
                    tokens: vec![api_key.to_string()],
                },
            })
            .await;
        match validate_result {
            Ok(token_status) => match token_status {
                TokenStatus::Valid(data) => {
                    println!("Had a valid token {data:#?}");
                    assert_eq!(data.token, api_key.to_string());
                }
                TokenStatus::Invalid => {
                    panic!("Had a valid but got an invalid status");
                }
            },
            Err(e) => {
                println!("{e:#?}");
                panic!("Invalid");
            }
        }
    }

    #[test]
    pub fn can_parse_entity_tag() {
        let etag = EntityTag::from_str("W/\"b5e6-DPC/1RShRw1J/jtxvRtTo1jf4+o\"").unwrap();
        assert!(etag.weak);
    }
}
