use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::io::BufReader;
use std::io::Read;
use std::path::PathBuf;
use std::str::FromStr;

use actix_web::http::header::EntityTag;
use chrono::Duration;
use chrono::Utc;
use lazy_static::lazy_static;
use prometheus::{register_histogram_vec, register_int_gauge_vec, HistogramVec, IntGaugeVec, Opts};
use reqwest::header::{HeaderMap, HeaderName};
use reqwest::{header, Client};
use reqwest::{ClientBuilder, Identity, RequestBuilder, StatusCode, Url};
use serde::{Deserialize, Serialize};
use tracing::{debug, error};
use tracing::{info, trace, warn};
use unleash_types::client_features::{ClientFeatures, ClientFeaturesDelta};
use unleash_types::client_metrics::ClientApplication;

use crate::cli::ClientIdentity;
use crate::error::EdgeError::EdgeMetricsRequestError;
use crate::error::{CertificateError, FeatureError};
use crate::http::headers::{
    UNLEASH_APPNAME_HEADER, UNLEASH_CLIENT_SPEC_HEADER, UNLEASH_INSTANCE_ID_HEADER,
};
use crate::metrics::client_metrics::MetricsBatch;
use crate::metrics::edge_metrics::EdgeInstanceData;
use crate::tls::build_upstream_certificate;
use crate::types::{
    ClientFeaturesDeltaResponse, ClientFeaturesResponse, EdgeResult, EdgeToken,
    TokenValidationStatus, ValidateTokensRequest,
};
use crate::urls::UnleashUrls;
use crate::{error::EdgeError, types::ClientFeaturesRequest};

lazy_static! {
    pub static ref CLIENT_REGISTER_FAILURES: IntGaugeVec = register_int_gauge_vec!(
        Opts::new(
            "client_register_failures",
            "Why we failed to register upstream"
        ),
        &["status_code", "app_name", "instance_id"]
    )
    .unwrap();
    pub static ref CLIENT_FEATURE_FETCH: HistogramVec = register_histogram_vec!(
        "client_feature_fetch",
        "Timings for fetching features in milliseconds",
        &["status_code", "app_name", "instance_id"],
        vec![1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 5000.0]
    )
    .unwrap();
    pub static ref CLIENT_FEATURE_DELTA_FETCH: HistogramVec = register_histogram_vec!(
        "client_feature_delta_fetch",
        "Timings for fetching feature deltas in milliseconds",
        &["status_code", "app_name", "instance_id"],
        vec![1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 5000.0]
    )
    .unwrap();
    pub static ref METRICS_UPLOAD: HistogramVec = register_histogram_vec!(
        "client_metrics_upload",
        "Timings for uploading client metrics in milliseconds",
        &["status_code", "app_name", "instance_id"],
        vec![1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0]
    )
    .unwrap();
    pub static ref INSTANCE_DATA_UPLOAD: HistogramVec = register_histogram_vec!(
        "instance_data_upload",
        "Timings for uploading Edge instance data in milliseconds",
        &["status_code", "app_name", "instance_id"],
        vec![1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0]
    )
    .unwrap();
    pub static ref CLIENT_FEATURE_FETCH_FAILURES: IntGaugeVec = register_int_gauge_vec!(
        Opts::new(
            "client_feature_fetch_failures",
            "Why we failed to fetch features"
        ),
        &["status_code", "app_name", "instance_id"]
    )
    .unwrap();
    pub static ref TOKEN_VALIDATION_FAILURES: IntGaugeVec = register_int_gauge_vec!(
        Opts::new(
            "token_validation_failures",
            "Why we failed to validate tokens"
        ),
        &["status_code", "app_name", "instance_id"]
    )
    .unwrap();
    pub static ref UPSTREAM_VERSION: IntGaugeVec = register_int_gauge_vec!(
        Opts::new(
            "upstream_version",
            "The server type (Unleash or Edge) and version of the upstream we're connected to"
        ),
        &["server", "version", "app_name", "instance_id"]
    )
    .unwrap();
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientMetaInformation {
    pub app_name: String,
    pub instance_id: String,
}

impl Default for ClientMetaInformation {
    fn default() -> Self {
        Self {
            app_name: "unleash-edge".into(),
            instance_id: format!("unleash-edge@{}", ulid::Ulid::new().to_string()),
        }
    }
}

impl ClientMetaInformation {
    pub fn test_config() -> Self {
        Self {
            app_name: "test-app-name".into(),
            instance_id: "test-instance-id".into(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct UnleashClient {
    pub urls: UnleashUrls,
    backing_client: Client,
    custom_headers: HashMap<String, String>,
    token_header: String,
    meta_info: ClientMetaInformation,
}

fn load_pkcs12(id: &ClientIdentity) -> EdgeResult<Identity> {
    let p12_file = fs::read(id.pkcs12_identity_file.clone().unwrap()).map_err(|e| {
        EdgeError::ClientCertificateError(CertificateError::Pkcs12ArchiveNotFound(format!("{e:?}")))
    })?;
    let p12_keystore =
        p12_keystore::KeyStore::from_pkcs12(&p12_file, &id.pkcs12_passphrase.clone().unwrap())
            .map_err(|e| {
                EdgeError::ClientCertificateError(CertificateError::Pkcs12ParseError(format!(
                    "{e:?}"
                )))
            })?;
    let mut pem = vec![];
    for (alias, entry) in p12_keystore.entries() {
        debug!("P12 entry: {alias}");
        match entry {
            p12_keystore::KeyStoreEntry::Certificate(_) => {
                info!("Direct Certificate, skipping. We want chain because client identity needs the private key");
            }
            p12_keystore::KeyStoreEntry::PrivateKeyChain(chain) => {
                let key_pem = pkix::pem::der_to_pem(chain.key(), pkix::pem::PEM_PRIVATE_KEY);
                pem.extend(key_pem.as_bytes());
                pem.push(0x0a); // Added new line
                for cert in chain.chain() {
                    let cert_pem = pkix::pem::der_to_pem(cert.as_der(), pkix::pem::PEM_CERTIFICATE);
                    pem.extend(cert_pem.as_bytes());
                    pem.push(0x0a); // Added new line
                }
            }
        }
    }

    Identity::from_pem(&pem).map_err(|e| {
        EdgeError::ClientCertificateError(CertificateError::Pkcs12X509Error(format!("{e:?}")))
    })
}

fn load_pkcs8_identity(id: &ClientIdentity) -> EdgeResult<Vec<u8>> {
    let cert = File::open(id.pkcs8_client_certificate_file.clone().unwrap()).map_err(|e| {
        EdgeError::ClientCertificateError(CertificateError::Pem8ClientCertNotFound(format!("{e:}")))
    })?;
    let key = File::open(id.pkcs8_client_key_file.clone().unwrap()).map_err(|e| {
        EdgeError::ClientCertificateError(CertificateError::Pem8ClientKeyNotFound(format!("{e:?}")))
    })?;
    let mut cert_reader = BufReader::new(cert);
    let mut key_reader = BufReader::new(key);
    let mut pem = vec![];
    let _ = key_reader.read_to_end(&mut pem);
    pem.push(0x0a);
    let _ = cert_reader.read_to_end(&mut pem);
    Ok(pem)
}

fn load_pkcs8(id: &ClientIdentity) -> EdgeResult<Identity> {
    Identity::from_pem(&load_pkcs8_identity(id)?).map_err(|e| {
        EdgeError::ClientCertificateError(CertificateError::Pem8IdentityGeneration(format!(
            "{e:?}"
        )))
    })
}

fn load_pem_identity(pem_file: PathBuf) -> EdgeResult<Vec<u8>> {
    let mut pem = vec![];
    let mut pem_reader = BufReader::new(File::open(pem_file).expect("No such file"));
    let _ = pem_reader.read_to_end(&mut pem);
    Ok(pem)
}

fn load_pem(id: &ClientIdentity) -> EdgeResult<Identity> {
    Identity::from_pem(&load_pem_identity(id.pem_cert_file.clone().unwrap())?).map_err(|e| {
        EdgeError::ClientCertificateError(CertificateError::Pem8IdentityGeneration(format!(
            "{e:?}"
        )))
    })
}

fn build_identity(tls: Option<ClientIdentity>) -> EdgeResult<ClientBuilder> {
    tls.map_or_else(
        || Ok(ClientBuilder::new().use_rustls_tls()),
        |tls| {
            let req_identity = if tls.pkcs12_identity_file.is_some() {
                // We're going to assume that we're using pkcs#12
                load_pkcs12(&tls)
            } else if tls.pkcs8_client_certificate_file.is_some() {
                load_pkcs8(&tls)
            } else if tls.pem_cert_file.is_some() {
                load_pem(&tls)
            } else {
                Err(EdgeError::ClientCertificateError(
                    CertificateError::NoCertificateFiles,
                ))
            };
            req_identity.map(|id| ClientBuilder::new().use_rustls_tls().identity(id))
        },
    )
}

pub fn new_reqwest_client(
    skip_ssl_verification: bool,
    client_identity: Option<ClientIdentity>,
    upstream_certificate_file: Option<PathBuf>,
    connect_timeout: Duration,
    socket_timeout: Duration,
    client_meta_information: ClientMetaInformation,
) -> EdgeResult<Client> {
    build_identity(client_identity)
        .and_then(|builder| {
            build_upstream_certificate(upstream_certificate_file).map(|cert| match cert {
                Some(c) => builder.add_root_certificate(c),
                None => builder,
            })
        })
        .and_then(|client| {
            let mut header_map = HeaderMap::new();
            header_map.insert(
                UNLEASH_APPNAME_HEADER,
                header::HeaderValue::from_str(&client_meta_information.app_name)
                    .expect("Could not add app name as a header"),
            );
            header_map.insert(
                UNLEASH_INSTANCE_ID_HEADER,
                header::HeaderValue::from_str(&client_meta_information.instance_id).unwrap(),
            );
            header_map.insert(
                UNLEASH_CLIENT_SPEC_HEADER,
                header::HeaderValue::from_static(unleash_yggdrasil::SUPPORTED_SPEC_VERSION),
            );

            client
                .user_agent(format!("unleash-edge-{}", crate::types::build::PKG_VERSION))
                .default_headers(header_map)
                .danger_accept_invalid_certs(skip_ssl_verification)
                .timeout(socket_timeout.to_std().unwrap())
                .connect_timeout(connect_timeout.to_std().unwrap())
                .build()
                .map_err(|e| EdgeError::ClientBuildError(format!("Failed to build client {e:?}")))
        })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EdgeTokens {
    pub tokens: Vec<EdgeToken>,
}

impl UnleashClient {
    pub fn from_url(
        server_url: Url,
        token_header: String,
        backing_client: Client,
        client_meta_information: ClientMetaInformation,
    ) -> Self {
        Self {
            urls: UnleashUrls::from_base_url(server_url),
            backing_client,
            custom_headers: Default::default(),
            token_header,
            meta_info: client_meta_information,
        }
    }

    pub fn new(server_url: &str, instance_id_opt: Option<String>) -> Result<Self, EdgeError> {
        use ulid::Ulid;

        let instance_id = instance_id_opt.unwrap_or_else(|| Ulid::new().to_string());
        let client_meta_info = ClientMetaInformation {
            instance_id,
            app_name: "test-client".into(),
        };
        Ok(Self {
            urls: UnleashUrls::from_str(server_url)?,
            backing_client: new_reqwest_client(
                false,
                None,
                None,
                Duration::seconds(5),
                Duration::seconds(5),
                client_meta_info.clone(),
            )
            .unwrap(),
            custom_headers: Default::default(),
            token_header: "Authorization".to_string(),
            meta_info: client_meta_info.clone(),
        })
    }

    #[cfg(test)]
    pub fn new_insecure(server_url: &str) -> Result<Self, EdgeError> {
        Ok(Self {
            urls: UnleashUrls::from_str(server_url)?,
            backing_client: new_reqwest_client(
                true,
                None,
                None,
                Duration::seconds(5),
                Duration::seconds(5),
                ClientMetaInformation::test_config(),
            )
            .unwrap(),
            custom_headers: Default::default(),
            token_header: "Authorization".to_string(),
            meta_info: ClientMetaInformation::test_config(),
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

    fn client_features_delta_req(&self, req: ClientFeaturesRequest) -> RequestBuilder {
        let client_req = self
            .backing_client
            .get(self.urls.client_features_delta_url.to_string())
            .headers(self.header_map(Some(req.api_key)));
        if let Some(tag) = req.etag {
            client_req.header(header::IF_NONE_MATCH, tag.to_string())
        } else {
            client_req
        }
    }

    fn header_map(&self, api_key: Option<String>) -> HeaderMap {
        let mut header_map = HeaderMap::new();
        let token_header: HeaderName = HeaderName::from_str(self.token_header.as_str()).unwrap();
        if let Some(key) = api_key {
            header_map.insert(token_header, key.parse().unwrap());
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
            .map_err(|e| {
                warn!("Failed to register client: {e:?}");
                EdgeError::ClientRegisterError
            })
            .map(|r| {
                if !r.status().is_success() {
                    CLIENT_REGISTER_FAILURES
                        .with_label_values(&[
                            r.status().as_str(),
                            &self.meta_info.app_name,
                            &self.meta_info.instance_id,
                        ])
                        .inc();
                    warn!(
                        "Failed to register client upstream with status code {}",
                        r.status()
                    );
                }
            })
    }

    pub async fn get_client_features(
        &self,
        request: ClientFeaturesRequest,
    ) -> EdgeResult<ClientFeaturesResponse> {
        let start_time = Utc::now();
        let response = self
            .client_features_req(request.clone())
            .send()
            .await
            .map_err(|e| {
                warn!("Failed to fetch. Due to [{e:?}] - Will retry");
                match e.status() {
                    Some(s) => EdgeError::ClientFeaturesFetchError(FeatureError::Retriable(s)),
                    None => EdgeError::ClientFeaturesFetchError(FeatureError::NotFound),
                }
            })?;
        let stop_time = Utc::now();
        CLIENT_FEATURE_FETCH
            .with_label_values(&[
                &response.status().as_u16().to_string(),
                &self.meta_info.app_name,
                &self.meta_info.instance_id,
            ])
            .observe(
                stop_time
                    .signed_duration_since(start_time)
                    .num_milliseconds() as f64,
            );
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
            let features = response.json::<ClientFeatures>().await.map_err(|e| {
                warn!("Could not parse features response to internal representation");
                EdgeError::ClientFeaturesParseError(e.to_string())
            })?;
            Ok(ClientFeaturesResponse::Updated(features, etag))
        } else if response.status() == StatusCode::FORBIDDEN {
            CLIENT_FEATURE_FETCH_FAILURES
                .with_label_values(&[
                    response.status().as_str(),
                    &self.meta_info.app_name,
                    &self.meta_info.instance_id,
                ])
                .inc();
            Err(EdgeError::ClientFeaturesFetchError(
                FeatureError::AccessDenied,
            ))
        } else if response.status() == StatusCode::UNAUTHORIZED {
            CLIENT_FEATURE_FETCH_FAILURES
                .with_label_values(&[
                    response.status().as_str(),
                    &self.meta_info.app_name,
                    &self.meta_info.instance_id,
                ])
                .inc();
            warn!(
                "Failed to get features. Url: [{}]. Status code: [401]",
                self.urls.client_features_url.to_string()
            );
            Err(EdgeError::ClientFeaturesFetchError(
                FeatureError::AccessDenied,
            ))
        } else if response.status() == StatusCode::NOT_FOUND {
            CLIENT_FEATURE_FETCH_FAILURES
                .with_label_values(&[
                    response.status().as_str(),
                    &self.meta_info.app_name,
                    &self.meta_info.instance_id,
                ])
                .inc();
            warn!(
                "Failed to get features. Url: [{}]. Status code: [{}]",
                self.urls.client_features_url.to_string(),
                response.status().as_str()
            );
            Err(EdgeError::ClientFeaturesFetchError(FeatureError::NotFound))
        } else {
            CLIENT_FEATURE_FETCH_FAILURES
                .with_label_values(&[
                    response.status().as_str(),
                    &self.meta_info.app_name,
                    &self.meta_info.instance_id,
                ])
                .inc();
            Err(EdgeError::ClientFeaturesFetchError(
                FeatureError::Retriable(response.status()),
            ))
        }
    }

    pub async fn get_client_features_delta(
        &self,
        request: ClientFeaturesRequest,
    ) -> EdgeResult<ClientFeaturesDeltaResponse> {
        let start_time = Utc::now();
        let response = self
            .client_features_delta_req(request.clone())
            .send()
            .await
            .map_err(|e| {
                warn!("Failed to fetch. Due to [{e:?}] - Will retry");
                match e.status() {
                    Some(s) => EdgeError::ClientFeaturesFetchError(FeatureError::Retriable(s)),
                    None => EdgeError::ClientFeaturesFetchError(FeatureError::NotFound),
                }
            })?;
        let stop_time = Utc::now();
        CLIENT_FEATURE_DELTA_FETCH
            .with_label_values(&[
                &response.status().as_u16().to_string(),
                &self.meta_info.app_name,
                &self.meta_info.instance_id,
            ])
            .observe(
                stop_time
                    .signed_duration_since(start_time)
                    .num_milliseconds() as f64,
            );
        if response.status() == StatusCode::NOT_MODIFIED {
            Ok(ClientFeaturesDeltaResponse::NoUpdate(
                request.etag.expect("Got NOT_MODIFIED without an ETag"),
            ))
        } else if response.status().is_success() {
            let etag = response
                .headers()
                .get("ETag")
                .or_else(|| response.headers().get("etag"))
                .and_then(|etag| EntityTag::from_str(etag.to_str().unwrap()).ok());
            let features = response.json::<ClientFeaturesDelta>().await.map_err(|e| {
                warn!("Could not parse features response to internal representation");
                EdgeError::ClientFeaturesParseError(e.to_string())
            })?;
            Ok(ClientFeaturesDeltaResponse::Updated(features, etag))
        } else if response.status() == StatusCode::FORBIDDEN {
            CLIENT_FEATURE_FETCH_FAILURES
                .with_label_values(&[response.status().as_str(), &self.meta_info.app_name])
                .inc();
            Err(EdgeError::ClientFeaturesFetchError(
                FeatureError::AccessDenied,
            ))
        } else if response.status() == StatusCode::UNAUTHORIZED {
            CLIENT_FEATURE_FETCH_FAILURES
                .with_label_values(&[response.status().as_str()])
                .inc();
            warn!(
                "Failed to get features. Url: [{}]. Status code: [401]",
                self.urls.client_features_delta_url.to_string()
            );
            Err(EdgeError::ClientFeaturesFetchError(
                FeatureError::AccessDenied,
            ))
        } else if response.status() == StatusCode::NOT_FOUND {
            CLIENT_FEATURE_FETCH_FAILURES
                .with_label_values(&[response.status().as_str()])
                .inc();
            warn!(
                "Failed to get features. Url: [{}]. Status code: [{}]",
                self.urls.client_features_delta_url.to_string(),
                response.status().as_str()
            );
            Err(EdgeError::ClientFeaturesFetchError(FeatureError::NotFound))
        } else {
            CLIENT_FEATURE_FETCH_FAILURES
                .with_label_values(&[response.status().as_str()])
                .inc();
            Err(EdgeError::ClientFeaturesFetchError(
                FeatureError::Retriable(response.status()),
            ))
        }
    }

    pub async fn send_batch_metrics(&self, request: MetricsBatch) -> EdgeResult<()> {
        trace!("Sending metrics to old /edge/metrics endpoint");
        let result = self
            .backing_client
            .post(self.urls.edge_metrics_url.to_string())
            .headers(self.header_map(None))
            .json(&request)
            .send()
            .await
            .map_err(|e| {
                info!("Failed to send batch metrics: {e:?}");
                EdgeError::EdgeMetricsError
            })?;
        if result.status().is_success() {
            Ok(())
        } else {
            match result.status() {
                StatusCode::BAD_REQUEST => Err(EdgeError::EdgeMetricsRequestError(
                    result.status(),
                    result.json().await.ok(),
                )),
                _ => Err(EdgeMetricsRequestError(result.status(), None)),
            }
        }
    }

    pub async fn send_bulk_metrics_to_client_endpoint(
        &self,
        request: MetricsBatch,
        token: &str,
    ) -> EdgeResult<()> {
        trace!("Sending metrics to bulk endpoint");
        let started_at = Utc::now();
        let result = self
            .backing_client
            .post(self.urls.client_bulk_metrics_url.to_string())
            .headers(self.header_map(Some(token.to_string())))
            .json(&request)
            .send()
            .await
            .map_err(|e| {
                info!("Failed to send metrics to /api/client/metrics/bulk endpoint {e:?}");
                EdgeError::EdgeMetricsError
            })?;
        let ended = Utc::now();
        METRICS_UPLOAD
            .with_label_values(&[
                result.status().as_str(),
                &self.meta_info.app_name,
                &self.meta_info.instance_id,
            ])
            .observe(ended.signed_duration_since(started_at).num_milliseconds() as f64);
        if result.status().is_success() {
            Ok(())
        } else {
            match result.status() {
                StatusCode::BAD_REQUEST => Err(EdgeMetricsRequestError(
                    result.status(),
                    result.json().await.ok(),
                )),
                _ => Err(EdgeMetricsRequestError(result.status(), None)),
            }
        }
    }

    #[tracing::instrument(skip(self, instance_data, token))]
    pub async fn post_edge_observability_data(
        &self,
        instance_data: EdgeInstanceData,
        token: &str,
    ) -> EdgeResult<()> {
        let started_at = Utc::now();
        let result = self
            .backing_client
            .post(self.urls.edge_instance_data_url.to_string())
            .headers(self.header_map(Some(token.into())))
            .timeout(Duration::seconds(3).to_std().unwrap())
            .json(&instance_data)
            .send()
            .await
            .map_err(|e| {
                info!("Failed to send instance data: {e:?}");
                EdgeError::EdgeMetricsError
            })?;
        let ended_at = Utc::now();
        INSTANCE_DATA_UPLOAD
            .with_label_values(&[
                result.status().as_str(),
                &self.meta_info.app_name,
                &self.meta_info.instance_id,
            ])
            .observe(
                ended_at
                    .signed_duration_since(started_at)
                    .num_milliseconds() as f64,
            );
        let r = if result.status().is_success() {
            Ok(())
        } else {
            match result.status() {
                StatusCode::BAD_REQUEST => Err(EdgeMetricsRequestError(
                    result.status(),
                    result.json().await.ok(),
                )),
                _ => Err(EdgeMetricsRequestError(result.status(), None)),
            }
        };
        debug!("Sent instance data to upstream server");
        r
    }

    pub async fn validate_tokens(
        &self,
        request: ValidateTokensRequest,
    ) -> EdgeResult<Vec<EdgeToken>> {
        let check_api_suffix = || {
            let base_url = self.urls.base_url.to_string();
            if base_url.ends_with("/api") || base_url.ends_with("/api/") {
                error!("Try passing the instance URL without '/api'.");
            }
        };

        let result = self
            .backing_client
            .post(self.urls.edge_validate_url.to_string())
            .headers(self.header_map(None))
            .json(&request)
            .send()
            .await
            .map_err(|e| {
                info!("Failed to validate tokens: [{e:?}]");
                EdgeError::EdgeTokenError
            })?;
        match result.status() {
            StatusCode::OK => {
                let token_response = result.json::<EdgeTokens>().await.map_err(|e| {
                    warn!("Failed to parse validation response with error: {e:?}");
                    EdgeError::EdgeTokenParseError
                })?;
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
            s => {
                TOKEN_VALIDATION_FAILURES
                    .with_label_values(&[
                        result.status().as_str(),
                        &self.meta_info.app_name,
                        &self.meta_info.instance_id,
                    ])
                    .inc();
                error!(
                    "Failed to validate tokens. Requested url: [{}]. Got status: {:?}",
                    self.urls.edge_validate_url.to_string(),
                    s
                );
                check_api_suffix();
                Err(EdgeError::TokenValidationError(
                    reqwest::StatusCode::from_u16(s.as_u16()).unwrap(),
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::str::FromStr;

    use crate::cli::ClientIdentity;
    use crate::http::unleash_client::new_reqwest_client;
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
    use chrono::Duration;
    use unleash_types::client_features::{ClientFeature, ClientFeatures};

    use super::{ClientMetaInformation, EdgeTokens, UnleashClient};

    impl ClientFeaturesRequest {
        pub(crate) fn new(api_key: String, etag: Option<String>) -> Self {
            Self {
                api_key,
                etag: etag.map(EntityTag::new_weak),
            }
        }
    }

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
            meta: None,
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
                    .wrap(Etag)
                    .service(
                        web::resource("/api/client/features")
                            .route(web::get().to(return_client_features)),
                    )
                    .service(
                        web::resource("/edge/validate")
                            .route(web::post().to(return_validate_tokens)),
                    )
                    .service(
                        web::resource("/api/edge/validate")
                            .route(web::post().to(HttpResponse::Forbidden)),
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
                TlsAcceptorConfig::default().handshake_timeout(std::time::Duration::from_secs(5));
            HttpService::new(map_config(
                App::new()
                    .wrap(Etag)
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
            .rustls_0_23_with_config(server_config, tls_acceptor_config)
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
                    .wrap(Etag)
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
            .map(|string| string.len())
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
                let validated_token = token_status.first().unwrap();
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

    #[cfg(target_os = "linux")]
    #[test]
    pub fn can_instantiate_pkcs_12_client() {
        let pfx = "./testdata/pkcs12/snakeoil.pfx";
        let passphrase = "password";
        let identity = ClientIdentity {
            pkcs8_client_certificate_file: None,
            pkcs8_client_key_file: None,
            pkcs12_identity_file: Some(PathBuf::from(pfx)),
            pkcs12_passphrase: Some(passphrase.into()),
            pem_cert_file: None,
        };
        let client = new_reqwest_client(
            false,
            Some(identity),
            None,
            Duration::seconds(5),
            Duration::seconds(5),
            ClientMetaInformation {
                app_name: "test-client".into(),
                instance_id: "test-pkcs12".into(),
            },
        );
        assert!(client.is_ok());
    }

    #[test]
    pub fn should_throw_error_if_wrong_passphrase_to_pfx_file() {
        let pfx = "./testdata/pkcs12/snakeoil.pfx";
        let passphrase = "wrongpassword";
        let identity = ClientIdentity {
            pkcs8_client_certificate_file: None,
            pkcs8_client_key_file: None,
            pkcs12_identity_file: Some(PathBuf::from(pfx)),
            pkcs12_passphrase: Some(passphrase.into()),
            pem_cert_file: None,
        };
        let client = new_reqwest_client(
            false,
            Some(identity),
            None,
            Duration::seconds(5),
            Duration::seconds(5),
            ClientMetaInformation {
                app_name: "test-client".into(),
                instance_id: "test-pkcs12".into(),
            },
        );
        assert!(client.is_err());
    }

    #[test]
    pub fn can_instantiate_pkcs_8_client() {
        let key = "./testdata/pkcs8/snakeoil.key.pem";
        let cert = "./testdata/pkcs8/snakeoil.crt";
        let identity = ClientIdentity {
            pkcs8_client_certificate_file: Some(cert.into()),
            pkcs8_client_key_file: Some(key.into()),
            pkcs12_identity_file: None,
            pkcs12_passphrase: None,
            pem_cert_file: None,
        };
        let client = new_reqwest_client(
            false,
            Some(identity),
            None,
            Duration::seconds(5),
            Duration::seconds(5),
            ClientMetaInformation {
                app_name: "test-client".into(),
                instance_id: "test-pkcs8".into(),
            },
        );
        assert!(client.is_ok());
    }

    #[test]
    pub fn can_instantiate_pem_client() {
        let cert = "./testdata/pem/keyStore.pem";
        let identity = ClientIdentity {
            pkcs8_client_certificate_file: None,
            pkcs8_client_key_file: None,
            pkcs12_identity_file: None,
            pkcs12_passphrase: None,
            pem_cert_file: Some(cert.into()),
        };
        let client = new_reqwest_client(
            false,
            Some(identity),
            None,
            Duration::seconds(5),
            Duration::seconds(5),
            ClientMetaInformation {
                app_name: "test-client".into(),
                instance_id: "test-pkcs8".into(),
            },
        );
        assert!(client.is_ok());
    }
}
