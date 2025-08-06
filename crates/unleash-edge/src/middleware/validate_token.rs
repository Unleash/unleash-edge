use axum::body::Body;
use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use reqwest::StatusCode;
use tracing::info;
use unleash_edge_appstate::AppState;
use unleash_edge_auth::token_validator::{TokenRegister, TokenValidator};
use unleash_edge_types::tokens::EdgeToken;
use unleash_edge_types::{TokenType, TokenValidationStatus};
use unleash_edge_types::errors::EdgeError;

pub async fn validate_token(app_state: State<AppState>, edge_token: EdgeToken, req: Request, next: Next) -> impl IntoResponse {
    let path = req.uri().path();
    let validation_status = match (*app_state.token_validator).clone() {
        Some(ref validator) => {
            validate_with_validator(&edge_token, path, validator).await
        }
        None => {
            validate_without_validator(app_state, &edge_token, path)
        }
    };
    match validation_status {
        Ok(_) => next.run(req).await,
        Err(err) => match err {
            EdgeError::AuthorizationDenied => Response::builder().status(StatusCode::UNAUTHORIZED).body(Body::empty()).unwrap(),
            EdgeError::Forbidden(_) => Response::builder().status(StatusCode::FORBIDDEN).body(Body::empty()).unwrap(),
            _ => err.into_response()
        }
    }
}

fn validate_without_validator(app_state: State<AppState>, edge_token: &EdgeToken, path: &str) -> Result<(), EdgeError> {
    match app_state.token_cache.get(&edge_token.token) {
        Some(t) => {
            let token = t.value();
            match token.token_type {
                Some(TokenType::Frontend) => check_frontend_path(path),
                Some(TokenType::Client) => check_backend_path(path),
                None => Ok(()),
                _ => Err(EdgeError::Forbidden("Unknown token type".into()))
            }
        }
        None => Err(EdgeError::Forbidden("No access allowed for token".into()))
    }
}

async fn validate_with_validator(edge_token: &EdgeToken, path: &str, validator: &TokenValidator) -> Result<(), EdgeError> {
    let known_token = validator.register_token(edge_token.token.clone()).await?;
    match known_token.status {
        TokenValidationStatus::Validated => match known_token.token_type {
            Some(TokenType::Frontend) => check_frontend_path(path),
            Some(TokenType::Client) => check_backend_path(path),
            _ => Err(EdgeError::Forbidden("".into()))
        }
        TokenValidationStatus::Unknown => Err(EdgeError::AuthorizationDenied),
        TokenValidationStatus::Invalid => Err(EdgeError::Forbidden("Token validation status was invalid".into())),
        TokenValidationStatus::Trusted => unreachable!()
    }
}

fn check_frontend_path(path: &str) -> Result<(), EdgeError> {
    if path.contains("/frontend") || path.contains("/proxy") {
        Ok(())
    } else {
        Err(EdgeError::Forbidden("".into()))
    }
}

fn check_backend_path(path: &str) -> Result<(), EdgeError> {
    if path.contains("/client") {
        Ok(())
    } else {
        Err(EdgeError::Forbidden("".into()))
    }
}