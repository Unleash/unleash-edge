use std::fmt::Display;

use actix_web::{http::StatusCode, HttpResponseBuilder, ResponseError};

#[derive(Debug)]
pub enum EdgeError {
    InvalidBackupFile(String),
    NoAuthorization,
    TlsError,
}

impl Display for EdgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EdgeError::InvalidBackupFile(msg) => write!(f, "{}", msg),
            EdgeError::NoAuthorization => write!(f, "You can't do that!"),
            EdgeError::TlsError => write!(f, "Could not configure TLS"),
        }
    }
}

impl ResponseError for EdgeError {
    fn status_code(&self) -> actix_web::http::StatusCode {
        match self {
            EdgeError::InvalidBackupFile(_) => StatusCode::INTERNAL_SERVER_ERROR,
            EdgeError::NoAuthorization => StatusCode::UNAUTHORIZED,
            EdgeError::TlsError => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> actix_web::HttpResponse<actix_web::body::BoxBody> {
        HttpResponseBuilder::new(self.status_code()).finish()
    }
}

#[cfg(test)]
mod tests {}
