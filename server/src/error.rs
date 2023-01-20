use std::error::Error;
use std::fmt::Display;

use actix_web::{http::StatusCode, HttpResponseBuilder, ResponseError};

#[derive(Debug)]
pub enum EdgeError {
    InvalidBackupFile(String, String),
    NoFeaturesFile,
    TlsError,
}

impl Error for EdgeError {}

impl Display for EdgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EdgeError::InvalidBackupFile(path, why_invalid) => write!(
                f,
                "file at path: {} was invalid due to {}",
                path, why_invalid
            ),
            EdgeError::TlsError => write!(f, "Could not configure TLS"),
            EdgeError::NoFeaturesFile => write!(f, "No features file located"),
        }
    }
}

impl ResponseError for EdgeError {
    fn status_code(&self) -> actix_web::http::StatusCode {
        match self {
            EdgeError::InvalidBackupFile(_, _) => StatusCode::INTERNAL_SERVER_ERROR,
            EdgeError::TlsError => StatusCode::INTERNAL_SERVER_ERROR,
            EdgeError::NoFeaturesFile => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> actix_web::HttpResponse<actix_web::body::BoxBody> {
        HttpResponseBuilder::new(self.status_code()).finish()
    }
}

#[cfg(test)]
mod tests {}
