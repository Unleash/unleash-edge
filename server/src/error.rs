use std::error::Error;
use std::fmt::{Display, Formatter};

use actix_web::{http::StatusCode, HttpResponseBuilder, ResponseError};
use serde::Serialize;
use serde_json::json;
use tracing::debug;

use crate::types::EdgeToken;

#[derive(Debug)]
pub enum FeatureError {
    AccessDenied,
    Retriable,
}

#[derive(Debug, Serialize)]
pub struct FrontendHydrationMissing {
    pub project: String,
    pub environment: String,
}

impl From<&EdgeToken> for FrontendHydrationMissing {
    fn from(value: &EdgeToken) -> Self {
        Self {
            project: value.projects.join(","),
            environment: value
                .environment
                .clone()
                .unwrap_or_else(|| "default".into()), // Should never hit or_else because we don't handle admin tokens
        }
    }
}

#[derive(Debug)]
pub enum CertificateError {
    Pkcs12ArchiveNotFound(String),
    Pkcs12IdentityGeneration(String),
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
        }
    }
}

#[derive(Debug)]
pub enum EdgeError {
    AuthorizationDenied,
    AuthorizationPending,
    ClientBuildError(String),
    ClientCertificateError(CertificateError),
    ClientFeaturesFetchError(FeatureError),
    ClientFeaturesParseError,
    ClientRegisterError,
    FrontendNotYetHydrated(FrontendHydrationMissing),
    FeatureNotFound(String),
    PersistenceError(String),
    EdgeMetricsError,
    EdgeMetricsRequestError(StatusCode),
    EdgeTokenError,
    EdgeTokenParseError,
    InvalidBackupFile(String, String),
    InvalidServerUrl(String),
    HealthCheckError(String),
    JsonParseError(String),
    NoFeaturesFile,
    NoTokenProvider,
    TlsError,
    TokenParseError(String),
    ContextParseError,
    ServiceAccountTokenNotEnabled,
}

impl Error for EdgeError {}

impl Display for EdgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EdgeError::InvalidBackupFile(path, why_invalid) => {
                write!(f, "file at path: {path} was invalid due to {why_invalid}")
            }
            EdgeError::TlsError => write!(f, "Could not configure TLS"),
            EdgeError::NoFeaturesFile => write!(f, "No features file located"),
            EdgeError::AuthorizationDenied => write!(f, "Not allowed to access"),
            EdgeError::NoTokenProvider => write!(f, "Could not get a TokenProvider"),
            EdgeError::TokenParseError(token) => write!(f, "Could not parse edge token: {token}"),
            EdgeError::PersistenceError(msg) => write!(f, "{msg}"),
            EdgeError::JsonParseError(msg) => write!(f, "{msg}"),
            EdgeError::ClientFeaturesFetchError(fe) => match fe {
                FeatureError::Retriable => write!(f, "Could not fetch client features. Will retry"),
                FeatureError::AccessDenied => write!(
                    f,
                    "Could not fetch client features because api key was not allowed"
                ),
            },
            EdgeError::FeatureNotFound(name) => {
                write!(f, "Failed to find feature with name {name}")
            }
            EdgeError::ClientFeaturesParseError => {
                write!(f, "Failed to parse client features")
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
            EdgeError::EdgeMetricsRequestError(status_code) => {
                write!(f, "Failed to post metrics with status code: {status_code}")
            }
            EdgeError::AuthorizationPending => {
                write!(f, "No validation for token has happened yet")
            }
            EdgeError::EdgeMetricsError => write!(f, "Edge metrics error"),
            EdgeError::FrontendNotYetHydrated(hydration_info) => {
                write!(f, "Edge not yet hydrated for {hydration_info:?}")
            }
            EdgeError::ContextParseError => {
                write!(f, "Failed to parse query parameters to frontend api")
            }
            EdgeError::ServiceAccountTokenNotEnabled => {
                write!(
                    f,
                    "No service account token was given at startup. Do not know how to proceed"
                )
            }
            EdgeError::HealthCheckError(message) => {
                write!(f, "{message}")
            }
        }
    }
}

impl ResponseError for EdgeError {
    fn status_code(&self) -> actix_web::http::StatusCode {
        match self {
            EdgeError::InvalidBackupFile(_, _) => StatusCode::INTERNAL_SERVER_ERROR,
            EdgeError::TlsError => StatusCode::INTERNAL_SERVER_ERROR,
            EdgeError::NoFeaturesFile => StatusCode::INTERNAL_SERVER_ERROR,
            EdgeError::AuthorizationDenied => StatusCode::FORBIDDEN,
            EdgeError::NoTokenProvider => StatusCode::INTERNAL_SERVER_ERROR,
            EdgeError::TokenParseError(_) => StatusCode::FORBIDDEN,
            EdgeError::ClientBuildError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            EdgeError::ClientFeaturesParseError => StatusCode::INTERNAL_SERVER_ERROR,
            EdgeError::ClientFeaturesFetchError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            EdgeError::InvalidServerUrl(_) => StatusCode::INTERNAL_SERVER_ERROR,
            EdgeError::PersistenceError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            EdgeError::JsonParseError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            EdgeError::EdgeTokenError => StatusCode::BAD_REQUEST,
            EdgeError::EdgeTokenParseError => StatusCode::BAD_REQUEST,
            EdgeError::AuthorizationPending => StatusCode::UNAUTHORIZED,
            EdgeError::FeatureNotFound(_) => StatusCode::NOT_FOUND,
            EdgeError::EdgeMetricsError => StatusCode::BAD_REQUEST,
            EdgeError::ClientRegisterError => StatusCode::BAD_REQUEST,
            EdgeError::ClientCertificateError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            EdgeError::FrontendNotYetHydrated(_) => StatusCode::NETWORK_AUTHENTICATION_REQUIRED,
            EdgeError::ContextParseError => StatusCode::BAD_REQUEST,
            EdgeError::ServiceAccountTokenNotEnabled => StatusCode::NETWORK_AUTHENTICATION_REQUIRED,
            EdgeError::EdgeMetricsRequestError(status_code) => *status_code,
            EdgeError::HealthCheckError(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> actix_web::HttpResponse<actix_web::body::BoxBody> {
        match self {
            EdgeError::FrontendNotYetHydrated(hydration_info) => {
                HttpResponseBuilder::new(self.status_code()).json(json!({
                    "explanation": "Edge does not yet have data for this token. Please make a call against /api/client/features with a client token that has the same access as your token",
                    "access": hydration_info
                }))
            },
            EdgeError::TokenParseError(token) => {
                debug!("Failed to parse token: {}", token);
                HttpResponseBuilder::new(self.status_code()).json(json!({
                    "explanation": format!("Edge could not parse token: {}", token),
                }))
            }
            _ => HttpResponseBuilder::new(self.status_code()).json(json!({
                "error": self.to_string()
            }))
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
