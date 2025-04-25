use crate::auth::token_validator::{TokenRegister, TokenValidator};

use actix_web::{
    HttpResponse,
    body::MessageBody,
    dev::{ServiceRequest, ServiceResponse},
    web::Data,
};
use dashmap::DashMap;
use unleash_edge_types::{EdgeToken, TokenType, TokenValidationStatus, error::EdgeError};

pub async fn validate_token(
    token: EdgeToken,
    req: ServiceRequest,
    srv: crate::middleware::as_async_middleware::Next<impl MessageBody + 'static>,
) -> Result<ServiceResponse<impl MessageBody>, actix_web::Error> {
    let maybe_validator = req.app_data::<Data<TokenValidator>>();
    let token_cache = req
        .app_data::<Data<DashMap<String, EdgeToken>>>()
        .unwrap()
        .clone()
        .into_inner();

    let validation_status = validate(&token, maybe_validator, &token_cache, req.path()).await;

    match validation_status {
        Ok(_) => Ok(srv.call(req).await?.map_into_left_body()),
        Err(err) => match err {
            EdgeError::AuthorizationDenied => Ok(req
                .into_response(HttpResponse::Unauthorized().finish())
                .map_into_right_body()),
            EdgeError::Forbidden(_) => Ok(req
                .into_response(HttpResponse::Forbidden().finish())
                .map_into_right_body()),
            _ => Err(err.into()),
        },
    }
}

async fn validate(
    token: &EdgeToken,
    maybe_validator: Option<&Data<impl TokenRegister>>,
    token_cache: &DashMap<String, EdgeToken>,
    path: &str,
) -> Result<(), EdgeError> {
    if token.status == TokenValidationStatus::Trusted {
        return Ok(());
    }

    match maybe_validator {
        Some(validator) => {
            let known_token = validator.register_token(token.token.clone()).await?;
            match known_token.status {
                TokenValidationStatus::Validated => match known_token.token_type {
                    Some(TokenType::Frontend) => check_frontend_path(path),
                    Some(TokenType::Client) => check_backend_path(path),
                    _ => Err(EdgeError::Forbidden("".into())),
                },

                TokenValidationStatus::Unknown => Err(EdgeError::AuthorizationDenied),
                TokenValidationStatus::Invalid => Err(EdgeError::Forbidden("".into())),
                TokenValidationStatus::Trusted => unreachable!(),
            }
        }
        None => match token_cache.get(&token.token) {
            Some(t) => {
                let token = t.value();
                match token.token_type {
                    Some(TokenType::Frontend) => check_frontend_path(path),
                    Some(TokenType::Client) => check_backend_path(path),
                    None => Ok(()),
                    _ => Err(EdgeError::Forbidden("".into())),
                }
            }
            None => Err(EdgeError::Forbidden("".into())),
        },
    }
}

fn check_frontend_path(path: &str) -> Result<(), EdgeError> {
    if path.contains("/api/frontend") || path.contains("/api/proxy") {
        Ok(())
    } else {
        Err(EdgeError::Forbidden("".into()))
    }
}

fn check_backend_path(path: &str) -> Result<(), EdgeError> {
    if path.contains("/api/client") {
        Ok(())
    } else {
        Err(EdgeError::Forbidden("".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;
    use unleash_edge_types::EdgeResult;

    struct FrontendValidator {}

    impl TokenRegister for FrontendValidator {
        async fn register_token(&self, token: String) -> EdgeResult<EdgeToken> {
            Ok(EdgeToken {
                status: TokenValidationStatus::Validated,
                token_type: Some(TokenType::Frontend),
                ..EdgeToken::from_str(&token).unwrap()
            })
        }
    }

    struct ClientValidator {}

    impl TokenRegister for ClientValidator {
        async fn register_token(&self, token: String) -> EdgeResult<EdgeToken> {
            Ok(EdgeToken {
                status: TokenValidationStatus::Validated,
                token_type: Some(TokenType::Client),
                ..EdgeToken::from_str(&token).unwrap()
            })
        }
    }

    struct FailValidator {}

    impl TokenRegister for FailValidator {
        async fn register_token(&self, _token: String) -> EdgeResult<EdgeToken> {
            Err(EdgeError::EdgeTokenError)
        }
    }

    #[actix_web::test]
    async fn validation_always_allows_trusted_tokens() {
        let token = EdgeToken {
            token: "legacy-123".into(),
            status: TokenValidationStatus::Trusted,
            token_type: Some(TokenType::Frontend),
            ..Default::default()
        };

        let result = validate(
            &token,
            Some(&Data::new(FailValidator {})),
            &DashMap::new(),
            "/api/frontend/some_path",
        )
        .await;

        assert!(result.is_ok());
    }

    #[actix_web::test]
    async fn validation_denies_frontend_tokens_on_backend_paths() {
        let token = EdgeToken {
            token: "*:development.somesecretstring".into(),
            status: TokenValidationStatus::Validated,
            token_type: Some(TokenType::Frontend),
            ..Default::default()
        };

        let hit_features = validate(
            &token,
            Some(&Data::new(FrontendValidator {})),
            &DashMap::new(),
            "/api/client/features",
        )
        .await;

        assert!(hit_features.is_err());
    }

    #[actix_web::test]
    async fn validation_allows_frontend_tokens_on_frontend_paths() {
        let token = EdgeToken {
            token: "*:development.somesecretstring".into(),
            status: TokenValidationStatus::Validated,
            token_type: Some(TokenType::Frontend),
            ..Default::default()
        };

        let hit_frontend = validate(
            &token,
            Some(&Data::new(FrontendValidator {})),
            &DashMap::new(),
            "/api/frontend",
        )
        .await;

        let hit_proxy = validate(
            &token,
            Some(&Data::new(FrontendValidator {})),
            &DashMap::new(),
            "/api/proxy",
        )
        .await;

        assert!(hit_frontend.is_ok());
        assert!(hit_proxy.is_ok());
    }

    #[actix_web::test]
    async fn validation_denies_client_tokens_on_frontend_paths() {
        let token = EdgeToken {
            token: "*:development.somesecretstring".into(),
            status: TokenValidationStatus::Validated,
            token_type: Some(TokenType::Client),
            ..Default::default()
        };

        let hit_frontend = validate(
            &token,
            Some(&Data::new(ClientValidator {})),
            &DashMap::new(),
            "/api/frontend",
        )
        .await;

        let hit_proxy = validate(
            &token,
            Some(&Data::new(ClientValidator {})),
            &DashMap::new(),
            "/api/proxy",
        )
        .await;
        assert!(hit_frontend.is_err());
        assert!(hit_proxy.is_err());
    }

    #[actix_web::test]
    async fn validation_allows_client_tokens_on_backend_paths() {
        let token = EdgeToken {
            token: "*:development.somesecretstring".into(),
            status: TokenValidationStatus::Validated,
            token_type: Some(TokenType::Client),
            ..Default::default()
        };

        let hit_features = validate(
            &token,
            Some(&Data::new(ClientValidator {})),
            &DashMap::new(),
            "/api/client/features",
        )
        .await;

        assert!(hit_features.is_ok());
    }

    #[actix_web::test]
    async fn broken_token_bubbles_error() {
        let token = EdgeToken {
            token: "totally-broken-token".into(),
            status: TokenValidationStatus::Invalid,
            ..Default::default()
        };

        let result = validate(
            &token,
            Some(&Data::new(FailValidator {})),
            &DashMap::new(),
            "/api/client/features",
        )
        .await;

        match result {
            Err(EdgeError::EdgeTokenError) => {}
            _ => panic!("Expected a token error"),
        }
    }
}
