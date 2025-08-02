use std::str::FromStr;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use unleash_edge_types::errors::EdgeError;
use unleash_edge_types::tokens::EdgeToken;
use crate::AppState;

impl FromRequestParts<AppState> for EdgeToken {
    type Rejection = EdgeError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        if let Some(edge_token) = parts.headers.get(state.auth_headers.edge_header_name())
            .and_then(|h| h.to_str().ok())
            .and_then(|t| EdgeToken::from_str(t).ok()) {
            Ok(edge_token)
        } else {
            Err(EdgeError::AuthorizationDenied)
        }
    }
}