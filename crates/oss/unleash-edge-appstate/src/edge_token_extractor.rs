use crate::AppState;
use axum::extract::{FromRef, FromRequestParts};
use axum::http::request::Parts;
use std::str::FromStr;
use std::sync::Arc;
use tracing::trace;
use unleash_edge_cli::AuthHeaders;
use unleash_edge_types::TokenCache;
use unleash_edge_types::errors::EdgeError;
use unleash_edge_types::tokens::EdgeToken;

pub struct AuthToken(pub EdgeToken);

#[derive(Clone)]
pub struct AuthState {
    pub auth_headers: AuthHeaders,
    pub token_cache: Arc<TokenCache>,
}

impl FromRef<AppState> for AuthState {
    fn from_ref(app: &AppState) -> Self {
        Self {
            auth_headers: app.auth_headers.clone(),
            token_cache: Arc::clone(&app.token_cache),
        }
    }
}

impl<S> FromRequestParts<S> for AuthToken
where
    S: Send + Sync,
    AuthState: FromRef<S>,
{
    type Rejection = EdgeError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let state = AuthState::from_ref(state);

        if let Some(edge_token) = parts
            .headers
            .get(state.auth_headers.edge_header_name())
            .and_then(|h| h.to_str().ok())
            .and_then(|t| EdgeToken::from_str(t).ok())
        {
            Ok(AuthToken(edge_token))
        } else {
            trace!("No extractable token in headers");
            Err(EdgeError::AuthorizationDenied)
        }
    }
}
