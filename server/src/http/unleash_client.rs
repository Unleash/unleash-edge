use actix_web::http::header::EntityTag;
use reqwest::{RequestBuilder, StatusCode, Url};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::time::Duration;
use ulid::Ulid;
use unleash_types::client_features::ClientFeatures;

use crate::types::{
    BatchMetricsRequestBody, ClientFeaturesResponse, EdgeResult, EdgeToken, TokenValidationStatus,
    ValidateTokensRequest,
};
use reqwest::{header, Client};

use crate::urls::UnleashUrls;
use crate::{error::EdgeError, types::ClientFeaturesRequest};

const UNLEASH_APPNAME_HEADER: &str = "UNLEASH-APPNAME";
const UNLEASH_INSTANCE_ID_HEADER: &str = "UNLEASH-INSTANCEID";
const UNLEASH_CLIENT_SPEC_HEADER: &str = "Unleash-Client-Spec";

#[derive(Clone, Debug, Default)]
pub struct UnleashClient {
    pub urls: UnleashUrls,
    backing_client: Client,
}

fn new_reqwest_client(instance_id: String) -> Client {
    let mut header_map = header::HeaderMap::new();
    header_map.insert(
        UNLEASH_APPNAME_HEADER,
        header::HeaderValue::from_static("unleash-edge"),
    );
    header_map.insert(
        UNLEASH_INSTANCE_ID_HEADER,
        header::HeaderValue::from_bytes(instance_id.as_bytes()).unwrap(),
    );
    header_map.insert(
        UNLEASH_CLIENT_SPEC_HEADER,
        header::HeaderValue::from_static(unleash_yggdrasil::SUPPORTED_SPEC_VERSION),
    );
    Client::builder()
        .user_agent(format!(
            "unleash-edge-{}",
            crate::types::build::PROJECT_NAME
        ))
        .default_headers(header_map)
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap()
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EdgeTokens {
    pub tokens: Vec<EdgeToken>,
}

impl UnleashClient {
    pub fn from_url(server_url: Url) -> Self {
        Self {
            urls: UnleashUrls::from_base_url(server_url),
            backing_client: new_reqwest_client("unleash_edge".into()),
        }
    }

    pub fn new(server_url: &str, instance_id_opt: Option<String>) -> Result<Self, EdgeError> {
        let instance_id = instance_id_opt.unwrap_or_else(|| Ulid::new().to_string());
        Ok(Self {
            urls: UnleashUrls::from_str(server_url)?,
            backing_client: new_reqwest_client(instance_id),
        })
    }

    fn client_features_req(&self, req: ClientFeaturesRequest) -> RequestBuilder {
        let client_req = self
            .backing_client
            .get(self.urls.client_features_url.to_string())
            .header(reqwest::header::AUTHORIZATION, req.api_key);
        if let Some(tag) = req.etag {
            client_req.header(reqwest::header::IF_NONE_MATCH, tag.to_string())
        } else {
            client_req
        }
    }

    pub async fn get_client_features(
        &self,
        request: ClientFeaturesRequest,
    ) -> EdgeResult<ClientFeaturesResponse> {
        let response = self
            .client_features_req(request.clone())
            .send()
            .await
            .map_err(|_| EdgeError::ClientFeaturesFetchError)?;
        if response.status() == StatusCode::NOT_MODIFIED {
            Ok(ClientFeaturesResponse::NoUpdate(
                request.etag.expect("Got NOT_MODIFIED without an ETag"),
            ))
        } else if response.status().is_success() {
            let etag = response
                .headers()
                .get("ETag")
                .and_then(|etag| EntityTag::from_str(etag.to_str().unwrap()).ok());
            let features = response
                .json::<ClientFeatures>()
                .await
                .map_err(|_e| EdgeError::ClientFeaturesParseError)?;
            Ok(ClientFeaturesResponse::Updated(features, etag))
        } else {
            Err(EdgeError::ClientFeaturesFetchError)
        }
    }

    pub async fn send_batch_metrics(&self, request: BatchMetricsRequestBody) -> EdgeResult<()> {
        let result = self
            .backing_client
            .post(self.urls.edge_metrics_url.to_string())
            .json(&request)
            .send()
            .await
            .map_err(|_| EdgeError::EdgeMetricsError)?;
        if result.status().is_success() {
            Ok(())
        } else {
            Err(EdgeError::EdgeMetricsError)
        }
    }

    pub async fn validate_tokens(
        &self,
        request: ValidateTokensRequest,
    ) -> EdgeResult<Vec<EdgeToken>> {
        let result = self
            .backing_client
            .post(self.urls.edge_validate_url.to_string())
            .json(&request)
            .send()
            .await
            .map_err(|_| EdgeError::EdgeTokenError)?;
        match result.status() {
            StatusCode::OK => {
                let token_response = result
                    .json::<EdgeTokens>()
                    .await
                    .map_err(|_| EdgeError::EdgeTokenParseError)?;
                Ok(token_response
                    .tokens
                    .into_iter()
                    .map(|t| {
                        let remaining_info =
                            EdgeToken::try_from(t.token.clone()).unwrap_or_else(|_| t.clone());
                        EdgeToken {
                            token: t.token.clone(),
                            token_type: t.token_type,
                            environment: t.environment.or(remaining_info.environment),
                            projects: t.projects,
                            status: TokenValidationStatus::Validated,
                        }
                    })
                    .collect())
            }
            _ => Err(EdgeError::EdgeTokenError),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::types::{
        ClientFeaturesRequest, ClientFeaturesResponse, EdgeToken, TokenValidationStatus,
        ValidateTokensRequest,
    };
    use actix_http::HttpService;
    use actix_http_test::{test_server, TestServer};
    use actix_middleware_etag::Etag;
    use actix_service::map_config;
    use actix_web::{dev::AppConfig, http::header::EntityTag, web, App, HttpResponse};
    use std::str::FromStr;
    use unleash_types::client_features::{ClientFeature, ClientFeatures};

    use super::{EdgeTokens, UnleashClient};

    const TEST_TOKEN: &str = "[]:development.08bce4267a3b1aa";

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

    async fn return_validate_tokens() -> HttpResponse {
        HttpResponse::Ok().json(EdgeTokens {
            tokens: vec![EdgeToken {
                token: TEST_TOKEN.into(),
                ..Default::default()
            }],
        })
    }

    async fn test_features_server() -> TestServer {
        test_server(move || {
            HttpService::new(map_config(
                App::new()
                    .wrap(Etag::default())
                    .service(
                        web::resource("/api/client/features")
                            .route(web::get().to(return_client_features)),
                    )
                    .service(
                        web::resource("/edge/validate")
                            .route(web::post().to(return_validate_tokens)),
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
        let srv = test_features_server().await;
        let tag = expected_etag(two_client_features());
        let client = UnleashClient::new(srv.url("/").as_str(), None).unwrap();
        let client_features_result = client
            .get_client_features(ClientFeaturesRequest::new(
                TEST_TOKEN.to_string(),
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
    async fn can_validate_token() {
        let srv = test_features_server().await;
        let client = UnleashClient::new(srv.url("/").as_str(), None).unwrap();
        let validate_result = client
            .validate_tokens(ValidateTokensRequest {
                tokens: vec![TEST_TOKEN.to_string()],
            })
            .await;
        match validate_result {
            Ok(token_status) => {
                assert_eq!(token_status.len(), 1);
                let validated_token = token_status.get(0).unwrap();
                assert_eq!(validated_token.status, TokenValidationStatus::Validated);
                assert_eq!(validated_token.environment, Some("development".into()))
            }
            Err(e) => {
                panic!("Error validating token: {e}");
            }
        }
    }

    #[test]
    pub fn can_parse_entity_tag() {
        let etag = EntityTag::from_str("W/\"b5e6-DPC/1RShRw1J/jtxvRtTo1jf4+o\"").unwrap();
        assert!(etag.weak);
    }
}
