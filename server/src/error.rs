use std::error::Error;
use std::fmt::Display;

use actix_web::{http::StatusCode, HttpResponseBuilder, ResponseError};
use awc::error::JsonPayloadError;

#[derive(Debug)]
pub enum EdgeError {
    AuthorizationDenied,
    AuthorizationPending,
    ClientFeaturesFetchError,
    ClientFeaturesParseError(JsonPayloadError),
    DataSourceError(String),
    EdgeTokenError,
    EdgeTokenParseError,
    InvalidBackupFile(String, String),
    InvalidServerUrl(String),
    JsonParseError(String),
    NoFeaturesFile,
    NoTokenProvider,
    TlsError,
    TokenParseError,
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
            EdgeError::TokenParseError => write!(f, "Could not parse edge token"),
            EdgeError::DataSourceError(msg) => write!(f, "{msg}"),
            EdgeError::JsonParseError(msg) => write!(f, "{msg}"),
            EdgeError::ClientFeaturesFetchError => {
                write!(f, "Could not fetch client features")
            }
            EdgeError::ClientFeaturesParseError(parse_error) => {
                write!(f, "Failed to parse client features: [{parse_error:#?}]")
            }
            EdgeError::InvalidServerUrl(msg) => write!(f, "Failed to parse server url: [{msg}]"),
            EdgeError::EdgeTokenError => write!(f, "Edge token error"),
            EdgeError::EdgeTokenParseError => write!(f, "Failed to parse token response"),
            EdgeError::AuthorizationPending => {
                write!(f, "No validation for token has happened yet")
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
            EdgeError::TokenParseError => StatusCode::FORBIDDEN,
            EdgeError::ClientFeaturesParseError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            EdgeError::ClientFeaturesFetchError => StatusCode::INTERNAL_SERVER_ERROR,
            EdgeError::InvalidServerUrl(_) => StatusCode::INTERNAL_SERVER_ERROR,
            EdgeError::DataSourceError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            EdgeError::JsonParseError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            EdgeError::EdgeTokenError => StatusCode::BAD_REQUEST,
            EdgeError::EdgeTokenParseError => StatusCode::BAD_REQUEST,
            EdgeError::AuthorizationPending => StatusCode::UNAUTHORIZED,
        }
    }

    fn error_response(&self) -> actix_web::HttpResponse<actix_web::body::BoxBody> {
        HttpResponseBuilder::new(self.status_code()).finish()
    }
}

impl From<serde_json::Error> for EdgeError {
    fn from(value: serde_json::Error) -> Self {
        EdgeError::JsonParseError(value.to_string())
    }
}

#[cfg(test)]
mod tests {}
