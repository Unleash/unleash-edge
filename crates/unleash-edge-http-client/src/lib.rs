use crate::tls::build_upstream_certificate;
use ahash::HashMap;
use axum::http::{HeaderName, StatusCode};
use chrono::{Duration, Utc};
use etag::EntityTag;
use lazy_static::lazy_static;
use prometheus::{HistogramVec, IntGaugeVec, Opts, register_histogram_vec, register_int_gauge_vec};
use reqwest::header::HeaderMap;
use reqwest::{Client, ClientBuilder, Identity, RequestBuilder, header};
use serde::{Deserialize, Serialize};
use std::fs;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::PathBuf;
use std::str::FromStr;
use tracing::{debug, error, info, trace, warn};
use ulid::Ulid;
use unleash_edge_cli::ClientIdentity;
use unleash_edge_types::enterprise::LicenseStateResponse;
use unleash_edge_types::errors::EdgeError::EdgeMetricsRequestError;
use unleash_edge_types::errors::{CertificateError, EdgeError, FeatureError};
use unleash_edge_types::headers::{
    UNLEASH_APPNAME_HEADER, UNLEASH_CLIENT_SPEC_HEADER, UNLEASH_CONNECTION_ID_HEADER,
    UNLEASH_INSTANCE_ID_HEADER, UNLEASH_INTERVAL,
};
use unleash_edge_types::metrics::batching::MetricsBatch;
use unleash_edge_types::metrics::instance_data::EdgeInstanceData;
use unleash_edge_types::tokens::EdgeToken;
use unleash_edge_types::urls::UnleashUrls;
use unleash_edge_types::{
    ClientFeaturesDeltaResponse, ClientFeaturesRequest, ClientFeaturesResponse, EdgeResult,
    EdgeTokens, TokenValidationStatus, ValidateTokensRequest, build,
};
use unleash_types::client_features::{ClientFeatures, ClientFeaturesDelta};
use unleash_types::client_metrics::ClientApplication;
use url::Url;

pub mod instance_data;
pub mod tls;

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
        vec![
            1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 5000.0
        ]
    )
    .unwrap();
    pub static ref CLIENT_FEATURE_DELTA_FETCH: HistogramVec = register_histogram_vec!(
        "client_feature_delta_fetch",
        "Timings for fetching feature deltas in milliseconds",
        &["status_code", "app_name", "instance_id"],
        vec![
            1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 5000.0
        ]
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

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeartbeatResponse {
    pub edge_license_state: LicenseStateResponse,
}

#[cfg_attr(test, derive(Default))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientMetaInformation {
    pub app_name: String,
    pub instance_id: Ulid,
    pub connection_id: Ulid,
}

#[cfg_attr(test, derive(Default))]
#[derive(Clone, Debug)]
pub struct HttpClientArgs {
    pub skip_ssl_verification: bool,
    pub client_identity: Option<ClientIdentity>,
    pub upstream_certificate_file: Option<PathBuf>,
    pub connect_timeout: Duration,
    pub socket_timeout: Duration,
    pub keep_alive_timeout: Duration,
    pub client_meta_information: ClientMetaInformation,
}

#[derive(Clone, Debug)]
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
                info!(
                    "Direct Certificate, skipping. We want chain because client identity needs the private key"
                );
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
            p12_keystore::KeyStoreEntry::Secret(_) => {
                info!(
                    "Direct secret, skipping. We want chain because client identity needs the private key"
                )
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

pub fn new_reqwest_client(args: HttpClientArgs) -> EdgeResult<Client> {
    build_identity(args.client_identity)
        .and_then(|builder| {
            build_upstream_certificate(args.upstream_certificate_file).map(|cert| match cert {
                Some(c) => builder.add_root_certificate(c),
                None => builder,
            })
        })
        .and_then(|client| {
            let mut header_map = HeaderMap::new();
            header_map.insert(
                UNLEASH_APPNAME_HEADER,
                header::HeaderValue::from_str(&args.client_meta_information.app_name)
                    .expect("Could not add app name as a header"),
            );
            header_map.insert(
                UNLEASH_INSTANCE_ID_HEADER,
                header::HeaderValue::from_str(
                    &args.client_meta_information.instance_id.to_string(),
                )
                .unwrap(),
            );
            header_map.insert(
                UNLEASH_CONNECTION_ID_HEADER,
                header::HeaderValue::from_str(
                    &args.client_meta_information.connection_id.to_string(),
                )
                .unwrap(),
            );
            header_map.insert(
                UNLEASH_CLIENT_SPEC_HEADER,
                header::HeaderValue::from_static(unleash_yggdrasil::SUPPORTED_SPEC_VERSION),
            );

            client
                .user_agent(format!("unleash-edge-{}", build::PKG_VERSION))
                .default_headers(header_map)
                .danger_accept_invalid_certs(args.skip_ssl_verification)
                .timeout(args.socket_timeout.to_std().unwrap())
                .connect_timeout(args.connect_timeout.to_std().unwrap())
                .tcp_keepalive(args.keep_alive_timeout.to_std().unwrap())
                .pool_idle_timeout(std::time::Duration::from_secs(60))
                .pool_max_idle_per_host(2)
                .build()
                .map_err(|e| EdgeError::ClientBuildError(format!("Failed to build client {e:?}")))
        })
}

fn redact_token_header(header_map: HeaderMap) -> HashMap<String, String> {
    header_map
        .iter()
        .map(|(k, v)| {
            if k.as_str().to_lowercase().contains("authorization") {
                let token = EdgeToken::try_from(v.clone());
                if let Ok(token) = token {
                    (k.as_str().to_string(), format!("{token:?}"))
                } else {
                    (k.as_str().to_string(), format!("{v:?}"))
                }
            } else {
                (
                    k.as_str().to_string(),
                    v.to_str().unwrap_or("Unknown header value").to_string(),
                )
            }
        })
        .collect::<HashMap<String, String>>()
}

impl UnleashClient {
    pub fn from_url_with_backing_client(
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

    #[cfg(test)]
    pub fn new_insecure(server_url: &str) -> Result<Self, EdgeError> {
        Ok(Self {
            urls: UnleashUrls::from_str(server_url)?,
            backing_client: new_reqwest_client(HttpClientArgs {
                skip_ssl_verification: true,
                client_meta_information: ClientMetaInformation::default(),
                ..Default::default()
            })
            .unwrap(),
            custom_headers: Default::default(),
            token_header: "Authorization".to_string(),
            meta_info: ClientMetaInformation::default(),
        })
    }

    fn client_features_req(&self, req: ClientFeaturesRequest) -> RequestBuilder {
        let mut client_req = self
            .backing_client
            .get(self.urls.client_features_url.to_string())
            .headers(self.header_map(Some(req.api_key)));

        if let Some(tag) = req.etag {
            client_req = client_req.header(header::IF_NONE_MATCH, tag.to_string());
        }

        if let Some(interval) = req.interval {
            client_req = client_req.header(UNLEASH_INTERVAL, interval.to_string());
        }

        client_req
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
                            &self.meta_info.instance_id.to_string(),
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
                &self.meta_info.instance_id.to_string(),
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
                    &self.meta_info.instance_id.to_string(),
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
                    &self.meta_info.instance_id.to_string(),
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
                    &self.meta_info.instance_id.to_string(),
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
                    &self.meta_info.instance_id.to_string(),
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
                &self.meta_info.instance_id.to_string(),
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
                .with_label_values(&[
                    response.status().as_str(),
                    &self.meta_info.app_name,
                    &self.meta_info.instance_id.to_string(),
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
                    &self.meta_info.instance_id.to_string(),
                ])
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
                .with_label_values(&[
                    response.status().as_str(),
                    &self.meta_info.app_name,
                    &self.meta_info.instance_id.to_string(),
                ])
                .inc();
            warn!(
                "Failed to get features. Url: [{}]. Status code: [{}]",
                self.urls.client_features_delta_url.to_string(),
                response.status().as_str()
            );
            Err(EdgeError::ClientFeaturesFetchError(FeatureError::NotFound))
        } else {
            CLIENT_FEATURE_FETCH_FAILURES
                .with_label_values(&[
                    response.status().as_str(),
                    &self.meta_info.app_name,
                    &self.meta_info.instance_id.to_string(),
                ])
                .inc();
            Err(EdgeError::ClientFeaturesFetchError(
                FeatureError::Retriable(response.status()),
            ))
        }
    }

    pub async fn send_heartbeat(&self, api_key: &EdgeToken) -> EdgeResult<()> {
        let response = self
            .backing_client
            .post(self.urls.heartbeat_url.to_string())
            .query(&[("connectionId", self.meta_info.connection_id)])
            .headers(self.header_map(Some(api_key.token.clone())))
            .send()
            .await
            .map_err(|e| {
                EdgeError::HeartbeatError(
                    format!("{e}"),
                    e.status().unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                )
            })?;

        match response.json::<HeartbeatResponse>().await {
            Ok(heartbeat_response) => match heartbeat_response.edge_license_state {
                LicenseStateResponse::Valid => Ok(()),
                LicenseStateResponse::Expired => Err(EdgeError::ExpiredLicense(
                    "License check failed: upstream reports the Enterprise Edge license is expired"
                        .into(),
                )),
                _ => Err(EdgeError::InvalidLicense(
                    "License check failed: upstream reports the Enterprise Edge license is invalid"
                        .into(),
                )),
            },
            Err(_) => Err(EdgeError::InvalidLicense(
                "License check failed: upstream could not verify the Enterprise Edge license"
                    .into(),
            )),
        }
    }

    pub async fn send_bulk_metrics_to_client_endpoint(
        &self,
        request: MetricsBatch,
        token: &str,
    ) -> EdgeResult<()> {
        trace!("Sending metrics to bulk endpoint");
        let started_at = Utc::now();
        let headers = self.header_map(Some(token.to_string()));
        debug!(
            "Using headers: {headers:?}",
            headers = redact_token_header(headers.clone())
        );
        let result = self
            .backing_client
            .post(self.urls.client_bulk_metrics_url.to_string())
            .headers(self.header_map(Some(token.to_string())))
            .json(&request)
            .send()
            .await
            .map_err(|e| {
                EdgeError::EdgeMetricsError(format!(
                    "Failed to send metrics to /api/client/metrics/bulk endpoint {e:?}"
                ))
            })?;
        let ended = Utc::now();
        METRICS_UPLOAD
            .with_label_values(&[
                result.status().as_str(),
                &self.meta_info.app_name,
                &self.meta_info.instance_id.to_string(),
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
                EdgeError::EdgeMetricsError(format!("Failed to send instance data: {e:?}"))
            })?;
        let ended_at = Utc::now();
        INSTANCE_DATA_UPLOAD
            .with_label_values(&[
                result.status().as_str(),
                &self.meta_info.app_name,
                &self.meta_info.instance_id.to_string(),
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
                        &self.meta_info.instance_id.to_string(),
                    ])
                    .inc();
                error!(
                    "Failed to validate tokens. Requested url: [{}]. Got status: {:?}",
                    self.urls.edge_validate_url.to_string(),
                    s
                );
                check_api_suffix();
                Err(EdgeError::TokenValidationError(
                    StatusCode::from_u16(s.as_u16()).unwrap(),
                ))
            }
        }
    }
}
