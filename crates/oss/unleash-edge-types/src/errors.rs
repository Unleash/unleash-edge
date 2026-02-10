use crate::tokens::EdgeToken;
use crate::{Status, UnleashBadRequest};
use axum::body::Body;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use redis::RedisError;
use serde::Serialize;
use serde_json::json;
use std::error::Error;
use std::fmt::{Display, Formatter};

pub const TRUST_PROXY_PARSE_ERROR: &str =
    "needs to be a valid ip address (ipv4 or ipv6) or a valid cidr (ipv4 or ipv6)";

#[derive(Debug, Clone)]
pub enum FeatureError {
    AccessDenied,
    NotFound,
    Retriable(reqwest::StatusCode),
}


impl From<RedisError> for EdgeError {
    fn from(err: RedisError) -> Self {
        EdgeError::PersistenceError(format!("Error connecting to Redis: {err}"))
    }
}

#[derive(Debug, Clone)]
pub enum CertificateError {
    Pkcs12ArchiveNotFound(String),
    Pkcs12IdentityGeneration(String),
    Pkcs12X509Error(String),
    Pkcs12ParseError(String),
    Pem8ClientKeyNotFound(String),
    Pem8ClientCertNotFound(String),
    Pem8IdentityGeneration(String),
    NoCertificateFiles,
    RootCertificatesError(String),
}

impl Display for CertificateError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            CertificateError::Pkcs12ArchiveNotFound(e) => {
                write!(f, "Failed to find pkcs12 archive. {e:?}")
            }
            CertificateError::Pkcs12IdentityGeneration(e) => {
                write!(
                    f,
                    "Failed to generate pkcs12 identity from parameters. {e:?}"
                )
            }
            CertificateError::Pem8ClientKeyNotFound(e) => {
                write!(f, "Failed to get pem8 client key. {e:?}")
            }
            CertificateError::Pem8ClientCertNotFound(e) => {
                write!(f, "Failed to get pem8 client cert. {e:?}")
            }
            CertificateError::Pem8IdentityGeneration(e) => {
                write!(
                    f,
                    "Failed to generate pkcs8 identity from parameters. {e:?}"
                )
            }
            CertificateError::NoCertificateFiles => {
                write!(
                    f,
                    "Could find neither a pfx file nor a pkcs#8 certificate. Aborting"
                )
            }
            CertificateError::RootCertificatesError(e) => {
                write!(f, "Could not load root certificate {e:?}")
            }
            CertificateError::Pkcs12ParseError(e) => {
                write!(f, "Failed to parse PKCS#12 archive {e:?}")
            }
            CertificateError::Pkcs12X509Error(e) => {
                write!(
                    f,
                    "Failed to read X509 certificate from PKCS#12 archive. {e:?}"
                )
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum EdgeError {
    AuthorizationDenied,
    AuthorizationPending,
    ClientBuildError(String),
    ClientCacheError,
    ClientCertificateError(CertificateError),
    ClientFeaturesFetchError(FeatureError),
    ClientFeaturesParseError(String),
    ClientHydrationFailed(String),
    ClientRegisterError,
    ContextParseError,
    EdgeMetricsError(String),
    EdgeMetricsRequestError(reqwest::StatusCode, Option<UnleashBadRequest>),
    EdgeTokenError,
    EdgeTokenParseError,
    FeatureNotFound(String),
    Forbidden(String),
    HealthCheckError(String),
    HeartbeatError(String, StatusCode),
    InvalidBackupFile(String, String),
    InvalidServerUrl(String),
    InvalidEtag,
    InvalidToken,
    JsonParseError(String),
    NoFeaturesFile,
    NoTokenProvider,
    NoTokens(String),
    NotReady,
    PersistenceError(String),
    ReadyCheckError(String),
    SseError(String),
    TlsError(String),
    TokenParseError(String),
    TokenValidationError(StatusCode),
    SocketBindError(String),
    TracingInitError(String),
}

impl Error for EdgeError {}

impl Display for EdgeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            EdgeError::InvalidBackupFile(path, why_invalid) => {
                write!(f, "file at path: {path} was invalid due to {why_invalid}")
            }
            EdgeError::TlsError(msg) => write!(f, "Could not configure TLS: {msg}"),
            EdgeError::NoFeaturesFile => write!(f, "No features file located"),
            EdgeError::AuthorizationDenied => write!(f, "Not allowed to access"),
            EdgeError::NoTokenProvider => write!(f, "Could not get a TokenProvider"),
            EdgeError::NoTokens(msg) => write!(f, "{msg}"),
            EdgeError::TokenParseError(token) => write!(f, "Could not parse edge token: {token}"),
            EdgeError::PersistenceError(msg) => write!(f, "{msg}"),
            EdgeError::JsonParseError(msg) => write!(f, "{msg}"),
            EdgeError::ClientFeaturesFetchError(fe) => match fe {
                FeatureError::Retriable(status_code) => write!(
                    f,
                    "Could not fetch client features. Will retry {status_code}"
                ),
                FeatureError::AccessDenied => write!(
                    f,
                    "Could not fetch client features because api key was not allowed"
                ),
                FeatureError::NotFound => write!(
                    f,
                    "Could not fetch features because upstream url was not found"
                ),
            },

            EdgeError::FeatureNotFound(name) => {
                write!(f, "Failed to find feature with name {name}")
            }
            EdgeError::ClientFeaturesParseError(error) => {
                write!(f, "Failed to parse client features: [{error}]")
            }
            EdgeError::ClientRegisterError => {
                write!(f, "Failed to register client")
            }
            EdgeError::ClientCertificateError(cert_error) => {
                write!(f, "Failed to build cert {cert_error:?}")
            }
            EdgeError::ClientBuildError(e) => write!(f, "Failed to build client {e:?}"),
            EdgeError::InvalidServerUrl(msg) => write!(f, "Failed to parse server url: [{msg}]"),
            EdgeError::EdgeTokenError => write!(f, "Edge token error"),
            EdgeError::EdgeTokenParseError => write!(f, "Failed to parse token response"),
            EdgeError::EdgeMetricsRequestError(status_code, message) => {
                write!(
                    f,
                    "Failed to post metrics with status code: {status_code} and response {message:?}"
                )
            }
            EdgeError::AuthorizationPending => {
                write!(f, "No validation for token has happened yet")
            }
            EdgeError::EdgeMetricsError(message) => write!(f, "Edge metrics error {message}"),
            EdgeError::ContextParseError => {
                write!(f, "Failed to parse query parameters to frontend api")
            }
            EdgeError::HealthCheckError(message) => {
                write!(f, "{message}")
            }
            EdgeError::HeartbeatError(message, status_code) => {
                write!(
                    f,
                    "Received status code {status_code} when trying to send heartbeat to upstream server: {message}"
                )
            }
            EdgeError::ReadyCheckError(message) => {
                write!(f, "{message}")
            }
            EdgeError::TokenValidationError(status_code) => {
                write!(
                    f,
                    "Received status code {} when trying to validate token against upstream server",
                    status_code
                )
            }
            EdgeError::ClientHydrationFailed(message) => {
                write!(
                    f,
                    "Client hydration failed. Somehow we said [{message}] when it did"
                )
            }
            EdgeError::ClientCacheError => {
                write!(f, "Fetching client features from cache failed")
            }
            EdgeError::NotReady => {
                write!(f, "Edge is not ready to serve requests")
            }
            EdgeError::InvalidToken => {
                write!(f, "The token is not subsumed by any registered tokens")
            }
            EdgeError::SseError(message) => write!(f, "{}", message),
            EdgeError::Forbidden(reason) => write!(f, "{}", reason),
            EdgeError::InvalidEtag => write!(f, "Failed to parse ETag header"),
            EdgeError::SocketBindError(msg) => {
                write!(f, "Failed to configure listening socket {msg}")
            }
            EdgeError::TracingInitError(s) => {
                write!(
                    f,
                    r#"Failed to instantiate tracing and logging (error message ${s}).
                    Please check the OTEL_EXPORTER_OTLP_ENDPOINT environment variable configuration and connectivity,
                    or unset it to disable the custom OpenTelemetry endpoint."#
                )
            }
        }
    }
}
impl IntoResponse for EdgeError {
    fn into_response(self) -> Response {
        match self.clone() {
            EdgeError::InvalidToken => Response::builder().status(self.status_code()).body(Body::empty()),
            EdgeError::SseError(_) => Response::builder().status(self.status_code()).body(Body::empty()),
            EdgeError::Forbidden(_) => Response::builder().status(self.status_code()).body(Body::empty()),
            EdgeError::AuthorizationDenied => Response::builder().status(self.status_code()).body(Body::empty()),
            EdgeError::AuthorizationPending => Response::builder().status(self.status_code()).body(Body::empty()),
            EdgeError::ClientBuildError(_) => Response::builder().status(self.status_code()).body(Body::empty()),
            EdgeError::ClientCacheError => Response::builder().status(self.status_code()).body(Body::empty()),
            EdgeError::ClientCertificateError(_) => Response::builder().status(self.status_code()).body(Body::empty()),
            EdgeError::ClientFeaturesFetchError(_) => Response::builder().status(self.status_code()).body(Body::empty()),
            EdgeError::ClientFeaturesParseError(_) => Response::builder().status(self.status_code()).body(Body::empty()),
            EdgeError::ClientHydrationFailed(_) => Response::builder().status(self.status_code()).body(Body::empty()),
            EdgeError::ClientRegisterError => Response::builder().status(self.status_code()).body(Body::empty()),
            EdgeError::ContextParseError => Response::builder().status(self.status_code()).body(Body::empty()),
            EdgeError::EdgeMetricsError(_) => Response::builder().status(self.status_code()).body(Body::empty()),
            EdgeError::EdgeMetricsRequestError(_, _) => Response::builder().status(self.status_code()).body(Body::empty()),
            EdgeError::EdgeTokenError => Response::builder().status(self.status_code()).body(Body::empty()),
            EdgeError::EdgeTokenParseError => Response::builder().status(self.status_code()).body(Body::empty()),
            EdgeError::FeatureNotFound(_) => Response::builder().status(self.status_code()).body(Body::empty()),
            EdgeError::HealthCheckError(_) => Response::builder().status(self.status_code()).body(Body::empty()),
            EdgeError::HeartbeatError(message, status_code) => Response::builder().status(self.status_code()).body(Body::from(json!({
                "explanation": format!("Received a non 200 status code when trying to validate token upstream: {message}"),
                "status_code": status_code.as_str()
            }).to_string())),
            EdgeError::InvalidBackupFile(_, _) => Response::builder().status(self.status_code()).body(Body::empty()),
            EdgeError::InvalidServerUrl(_) => Response::builder().status(self.status_code()).body(Body::empty()),
            EdgeError::JsonParseError(_) => Response::builder().status(self.status_code()).body(Body::empty()),
            EdgeError::NoFeaturesFile => Response::builder().status(self.status_code()).body(Body::empty()),
            EdgeError::NoTokenProvider => Response::builder().status(self.status_code()).body(Body::empty()),
            EdgeError::NoTokens(_) => Response::builder().status(self.status_code()).body(Body::empty()),
            EdgeError::NotReady => Response::builder().status(self.status_code()).body(Body::from(json!({
                "error": "Edge is not ready to serve requests",
                "status": Status::NotReady
            }).to_string())),
            EdgeError::PersistenceError(_) => Response::builder().status(self.status_code()).body(Body::empty()),
            EdgeError::ReadyCheckError(_) => Response::builder().status(self.status_code()).body(Body::empty()),
            EdgeError::TlsError(_) => Response::builder().status(self.status_code()).body(Body::empty()),
            EdgeError::TokenParseError(token) => Response::builder().status(self.clone().status_code()).body(Body::from(json!({
                "explanation": format!("Edge could not parse token {}", token.clone())
            }).to_string())),
            EdgeError::TokenValidationError(status_code) => Response::builder().status(self.status_code()).body(Body::from(json!({
                "explanation": format!("Received a non 200 status code when trying to validate token upstream"),
                "status_code": status_code.as_str()
            }).to_string())),
            EdgeError::InvalidEtag => Response::builder().status(self.status_code()).body(Body::from(json!({
                "explanation": "Failed to parse ETag header"
            }).to_string())),
            EdgeError::SocketBindError(_) => Response::builder().status(self.status_code()).body(Body::empty()),
            EdgeError::TracingInitError(msg) => Response::builder().status(self.status_code()).body(Body::from(json!({
                "explanation": format!("Failed to instantiate tracing with error message: {msg}")
            }).to_string()))
        }.expect("Failed to build response")
    }
}
impl EdgeError {
    pub fn status_code(&self) -> StatusCode {
        match self {
            EdgeError::InvalidBackupFile(_, _) => StatusCode::INTERNAL_SERVER_ERROR,
            EdgeError::TlsError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            EdgeError::NoFeaturesFile => StatusCode::INTERNAL_SERVER_ERROR,
            EdgeError::AuthorizationDenied => StatusCode::FORBIDDEN,
            EdgeError::NoTokenProvider => StatusCode::INTERNAL_SERVER_ERROR,
            EdgeError::NoTokens(_) => StatusCode::INTERNAL_SERVER_ERROR,
            EdgeError::TokenParseError(_) => StatusCode::FORBIDDEN,
            EdgeError::ClientBuildError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            EdgeError::ClientFeaturesParseError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            EdgeError::ClientFeaturesFetchError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            EdgeError::InvalidServerUrl(_) => StatusCode::INTERNAL_SERVER_ERROR,
            EdgeError::PersistenceError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            EdgeError::JsonParseError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            EdgeError::EdgeTokenError => StatusCode::BAD_REQUEST,
            EdgeError::EdgeTokenParseError => StatusCode::BAD_REQUEST,
            EdgeError::TokenValidationError(status_code) => *status_code,
            EdgeError::AuthorizationPending => StatusCode::UNAUTHORIZED,
            EdgeError::FeatureNotFound(_) => StatusCode::NOT_FOUND,
            EdgeError::EdgeMetricsError(_) => StatusCode::BAD_REQUEST,
            EdgeError::ClientRegisterError => StatusCode::BAD_REQUEST,
            EdgeError::ClientCertificateError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            EdgeError::ContextParseError => StatusCode::BAD_REQUEST,
            EdgeError::EdgeMetricsRequestError(status_code, _) => {
                StatusCode::from_u16(status_code.as_u16()).unwrap()
            }
            EdgeError::HealthCheckError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            EdgeError::HeartbeatError(_, status_code) => *status_code,
            EdgeError::ReadyCheckError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            EdgeError::ClientHydrationFailed(_) => StatusCode::INTERNAL_SERVER_ERROR,
            EdgeError::ClientCacheError => StatusCode::INTERNAL_SERVER_ERROR,
            EdgeError::NotReady => StatusCode::SERVICE_UNAVAILABLE,
            EdgeError::InvalidToken => StatusCode::FORBIDDEN,
            EdgeError::SseError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            EdgeError::Forbidden(_) => StatusCode::FORBIDDEN,
            &EdgeError::InvalidEtag => StatusCode::BAD_REQUEST,
            EdgeError::SocketBindError(_) => StatusCode::SERVICE_UNAVAILABLE,
            EdgeError::TracingInitError(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl From<serde_json::Error> for EdgeError {
    fn from(value: serde_json::Error) -> Self {
        EdgeError::JsonParseError(value.to_string())
    }
}

#[cfg(test)]
mod tests {}
