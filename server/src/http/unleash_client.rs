use actix_web::http::header::EntityTag;
use lazy_static::lazy_static;
use reqwest::header::{HeaderMap, HeaderName};
use reqwest::{RequestBuilder, StatusCode, Url};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;
use std::time::Duration;
use unleash_types::client_features::ClientFeatures;

use crate::metrics::client_metrics::MetricsBatch;
use crate::types::{
    ClientFeaturesResponse, EdgeResult, EdgeToken, TokenValidationStatus, ValidateTokensRequest,
};

use prometheus::{register_int_gauge_vec, IntGaugeVec, Opts};
use reqwest::{header, Client};
use unleash_types::client_metrics::ClientApplication;

use crate::error::FeatureError;
use crate::urls::UnleashUrls;
use crate::{error::EdgeError, types::ClientFeaturesRequest};

const UNLEASH_APPNAME_HEADER: &str = "UNLEASH-APPNAME";
const UNLEASH_INSTANCE_ID_HEADER: &str = "UNLEASH-INSTANCEID";
const UNLEASH_CLIENT_SPEC_HEADER: &str = "Unleash-Client-Spec";

lazy_static! {
    pub static ref CLIENT_REGISTER_FAILURES: IntGaugeVec = register_int_gauge_vec!(
        Opts::new(
            "client_register_failures",
            "Why we failed to register upstream"
        ),
        &["status_code"]
    )
    .unwrap();
    pub static ref CLIENT_FEATURE_FETCH_FAILURES: IntGaugeVec = register_int_gauge_vec!(
        Opts::new(
            "client_feature_fetch_failures",
            "Why we failed to fetch features"
        ),
        &["status_code"]
    )
    .unwrap();
    pub static ref TOKEN_VALIDATION_FAILURES: IntGaugeVec = register_int_gauge_vec!(
        Opts::new(
            "token_validation_failures",
            "Why we failed to validate tokens"
        ),
        &["status_code"]
    )
    .unwrap();
}

#[derive(Clone, Debug, Default)]
pub struct UnleashClient {
    pub urls: UnleashUrls,
    backing_client: Client,
    custom_headers: HashMap<String, String>,
}

fn new_reqwest_client(instance_id: String, skip_ssl_verification: bool) -> Client {
    let mut header_map = HeaderMap::new();
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
        .user_agent(format!("unleash-edge-{}", crate::types::build::PKG_VERSION))
        .default_headers(header_map)
        .danger_accept_invalid_certs(skip_ssl_verification)
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap()
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EdgeTokens {
    pub tokens: Vec<EdgeToken>,
}

impl UnleashClient {
    pub fn from_url(server_url: Url, skip_ssl_verification: bool) -> Self {
        Self {
            urls: UnleashUrls::from_base_url(server_url),
            backing_client: new_reqwest_client("unleash_edge".into(), skip_ssl_verification),
            custom_headers: Default::default(),
        }
    }

    #[cfg(test)]
    pub fn new(server_url: &str, instance_id_opt: Option<String>) -> Result<Self, EdgeError> {
        use ulid::Ulid;

        let instance_id = instance_id_opt.unwrap_or_else(|| Ulid::new().to_string());
        Ok(Self {
            urls: UnleashUrls::from_str(server_url)?,
            backing_client: new_reqwest_client(instance_id, false),
            custom_headers: Default::default(),
        })
    }

    #[cfg(test)]
    pub fn new_insecure(server_url: &str) -> Result<Self, EdgeError> {
        use ulid::Ulid;

        Ok(Self {
            urls: UnleashUrls::from_str(server_url)?,
            backing_client: new_reqwest_client(Ulid::new().to_string(), true),
            custom_headers: Default::default(),
        })
    }

    fn client_features_req(&self, req: ClientFeaturesRequest) -> RequestBuilder {
        let client_req = self
            .backing_client
            .get(self.urls.client_features_url.to_string())
            .headers(self.header_map(Some(req.api_key)));
        if let Some(tag) = req.etag {
            client_req.header(header::IF_NONE_MATCH, tag.to_string())
        } else {
            client_req
        }
    }

    fn header_map(&self, api_key: Option<String>) -> HeaderMap {
        let mut header_map = HeaderMap::new();
        if let Some(key) = api_key {
            header_map.insert(header::AUTHORIZATION, key.parse().unwrap());
        }
        for (header_name, header_value) in self.custom_headers.iter() {
            let key = HeaderName::from_str(header_name.as_str()).unwrap();
            header_map.insert(key, header_value.parse().unwrap());
        }
        header_map
    }

    pub fn with_custom_client_headers(self, custom_headers: Vec<(String, String)>) -> Self {
        Self {
            custom_headers: custom_headers.iter().cloned().collect(),
            ..self
        }
    }

    pub async fn register_as_client(
        &self,
        api_key: String,
        application: ClientApplication,
    ) -> EdgeResult<()> {
        self.backing_client
            .post(self.urls.client_register_app_url.to_string())
            .headers(self.header_map(Some(api_key)))
            .json(&application)
            .send()
            .await
            .map_err(|_| EdgeError::ClientRegisterError)
            .map(|r| {
                if !r.status().is_success() {
                    CLIENT_REGISTER_FAILURES
                        .with_label_values(&[r.status().as_str()])
                        .inc()
                }
            })
    }

    pub async fn get_client_features(
        &self,
        request: ClientFeaturesRequest,
    ) -> EdgeResult<ClientFeaturesResponse> {
        let response = self
            .client_features_req(request.clone())
            .send()
            .await
            .map_err(|_| EdgeError::ClientFeaturesFetchError(FeatureError::Retriable))?;
        if response.status() == StatusCode::NOT_MODIFIED {
            Ok(ClientFeaturesResponse::NoUpdate(
                request.etag.expect("Got NOT_MODIFIED without an ETag"),
            ))
        } else if response.status().is_success() {
            let etag = response
                .headers()
                .get("ETag")
                .or_else(|| response.headers().get("etag"))
                .and_then(|etag| EntityTag::from_str(etag.to_str().unwrap()).ok());
            let features = response
                .json::<ClientFeatures>()
                .await
                .map_err(|_e| EdgeError::ClientFeaturesParseError)?;
            Ok(ClientFeaturesResponse::Updated(features, etag))
        } else if response.status() == StatusCode::FORBIDDEN {
            CLIENT_FEATURE_FETCH_FAILURES
                .with_label_values(&[response.status().as_str()])
                .inc();
            Err(EdgeError::ClientFeaturesFetchError(
                FeatureError::AccessDenied,
            ))
        } else {
            CLIENT_FEATURE_FETCH_FAILURES
                .with_label_values(&[response.status().as_str()])
                .inc();
            Err(EdgeError::ClientFeaturesFetchError(FeatureError::Retriable))
        }
    }

    pub async fn send_batch_metrics(&self, request: MetricsBatch) -> EdgeResult<()> {
        let result = self
            .backing_client
            .post(self.urls.edge_metrics_url.to_string())
            .headers(self.header_map(None))
            .json(&request)
            .send()
            .await
            .map_err(|_| EdgeError::EdgeMetricsError)?;
        if result.status().is_success() {
            Ok(())
        } else {
            Err(EdgeError::EdgeMetricsRequestError(result.status()))
        }
    }

    pub async fn validate_tokens(
        &self,
        request: ValidateTokensRequest,
    ) -> EdgeResult<Vec<EdgeToken>> {
        let result = self
            .backing_client
            .post(self.urls.edge_validate_url.to_string())
            .headers(self.header_map(None))
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
            _ => {
                TOKEN_VALIDATION_FAILURES
                    .with_label_values(&[result.status().as_str()])
                    .inc();
                Err(EdgeError::EdgeTokenError)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        cli::TlsOptions,
        middleware::as_async_middleware::as_async_middleware,
        tls,
        types::{
            ClientFeaturesRequest, ClientFeaturesResponse, EdgeToken, TokenValidationStatus,
            ValidateTokensRequest,
        },
    };
    use actix_http::{body::MessageBody, HttpService, TlsAcceptorConfig};
    use actix_http_test::{test_server, TestServer};
    use actix_middleware_etag::Etag;
    use actix_service::map_config;
    use actix_web::{
        dev::{AppConfig, ServiceRequest, ServiceResponse},
        http::header::EntityTag,
        web, App, HttpResponse,
    };
    use std::{str::FromStr, time::Duration};
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

    async fn test_features_server_with_untrusted_ssl() -> TestServer {
        test_server(move || {
            let tls_options = TlsOptions {
                tls_server_cert: Some("../examples/server.crt".into()),
                tls_enable: true,
                tls_server_key: Some("../examples/server.key".into()),
                tls_server_port: 443,
            };
            let server_config = tls::config(tls_options).unwrap();
            let tls_acceptor_config =
                TlsAcceptorConfig::default().handshake_timeout(Duration::from_secs(5));
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
            .rustls_with_config(server_config, tls_acceptor_config)
        })
        .await
    }

    async fn validate_api_key_middleware(
        req: ServiceRequest,
        srv: crate::middleware::as_async_middleware::Next<impl MessageBody + 'static>,
    ) -> Result<ServiceResponse<impl MessageBody>, actix_web::Error> {
        let res = if req
            .headers()
            .get("X-Api-Key")
            .map(|key| key.to_str().unwrap() == "MyMagicKey")
            .unwrap_or(false)
        {
            srv.call(req).await?.map_into_left_body()
        } else {
            req.into_response(HttpResponse::Forbidden().finish())
                .map_into_right_body()
        };
        Ok(res)
    }

    async fn test_features_server_with_required_custom_header() -> TestServer {
        test_server(move || {
            HttpService::new(map_config(
                App::new()
                    .wrap(Etag::default())
                    .wrap(as_async_middleware(validate_api_key_middleware))
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

    #[test]
    pub fn parse_entity_tag() {
        let optimal_304_tag = EntityTag::from_str("\"76d8bb0e:2841\"");
        assert!(optimal_304_tag.is_ok());
    }

    #[actix_web::test]
    pub async fn custom_client_headers_are_sent_along() {
        let custom_headers = vec![("X-Api-Key".to_string(), "MyMagicKey".to_string())];
        let srv = test_features_server_with_required_custom_header().await;
        let client_without_extra_headers = UnleashClient::new(srv.url("/").as_str(), None).unwrap();
        let client_with_headers = client_without_extra_headers
            .clone()
            .with_custom_client_headers(custom_headers);
        let res = client_without_extra_headers
            .get_client_features(ClientFeaturesRequest {
                api_key: "notneeded".into(),
                etag: None,
            })
            .await;
        assert!(res.is_err());
        let authed_res = client_with_headers
            .get_client_features(ClientFeaturesRequest {
                api_key: "notneeded".into(),
                etag: None,
            })
            .await;
        assert!(authed_res.is_ok());
    }

    #[actix_web::test]
    pub async fn disabling_ssl_verification_allows_communicating_with_upstream_unleash_with_self_signed_cert(
    ) {
        let srv = test_features_server_with_untrusted_ssl().await;
        let client = UnleashClient::new_insecure(srv.surl("/").as_str()).unwrap();

        let validate_result = client
            .validate_tokens(ValidateTokensRequest {
                tokens: vec![TEST_TOKEN.to_string()],
            })
            .await;

        assert!(validate_result.is_ok());
    }

    #[actix_web::test]
    pub async fn not_disabling_ssl_verification_fails_communicating_with_upstream_unleash_with_self_signed_cert(
    ) {
        let srv = test_features_server_with_untrusted_ssl().await;
        let client = UnleashClient::new(srv.surl("/").as_str(), None).unwrap();

        let validate_result = client
            .validate_tokens(ValidateTokensRequest {
                tokens: vec![TEST_TOKEN.to_string()],
            })
            .await;

        assert!(validate_result.is_err());
    }
}
