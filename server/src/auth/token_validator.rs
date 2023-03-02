use crate::error::EdgeError;
use crate::http::unleash_client::UnleashClient;
use crate::types::{EdgeResult, EdgeToken, ValidateTokensRequest};
use std::sync::Arc;

use dashmap::DashMap;
use unleash_types::Upsert;
#[derive(Clone)]
pub struct TokenValidator {
    pub unleash_client: Arc<UnleashClient>,
    pub token_cache: Arc<DashMap<String, EdgeToken>>,
}

impl TokenValidator {
    async fn get_unknown_and_known_tokens(
        &self,
        tokens: Vec<String>,
    ) -> EdgeResult<(Vec<EdgeToken>, Vec<EdgeToken>)> {
        let tokens_with_valid_format: Vec<EdgeToken> = tokens
            .into_iter()
            .filter_map(|t| EdgeToken::try_from(t).ok())
            .collect();

        if tokens_with_valid_format.is_empty() {
            Err(EdgeError::TokenParseError)
        } else {
            let mut tokens: Vec<EdgeToken> = vec![];
            for token in tokens_with_valid_format {
                let owned_token = self
                    .token_cache
                    .get(&token.token.clone())
                    .map(|t| t.value().clone())
                    .unwrap_or_else(|| token.clone());
                tokens.push(owned_token);
            }
            Ok(tokens.into_iter().partition(|t| t.token_type.is_none()))
        }
    }

    pub async fn register_token(&self, token: String) -> EdgeResult<EdgeToken> {
        Ok(self
            .register_tokens(vec![token])
            .await?
            .first()
            .expect("Couldn't validate token")
            .clone())
    }

    pub async fn register_tokens(&self, tokens: Vec<String>) -> EdgeResult<Vec<EdgeToken>> {
        let (unknown_tokens, known_tokens) = self.get_unknown_and_known_tokens(tokens).await?;
        if unknown_tokens.is_empty() {
            Ok(known_tokens)
        } else {
            let token_strings_to_validate: Vec<String> =
                unknown_tokens.iter().map(|t| t.token.clone()).collect();

            let validation_result = self
                .unleash_client
                .validate_tokens(ValidateTokensRequest {
                    tokens: token_strings_to_validate,
                })
                .await?;

            let tokens_to_sink: Vec<EdgeToken> = unknown_tokens
                .into_iter()
                .map(|maybe_valid| {
                    if let Some(validated_token) = validation_result
                        .iter()
                        .find(|v| maybe_valid.token == v.token)
                    {
                        EdgeToken {
                            status: crate::types::TokenValidationStatus::Validated,
                            ..validated_token.clone()
                        }
                    } else {
                        EdgeToken {
                            status: crate::types::TokenValidationStatus::Invalid,
                            ..maybe_valid
                        }
                    }
                })
                .collect();
            tokens_to_sink.iter().for_each(|t| {
                self.token_cache.insert(t.token.clone(), t.clone());
            });
            Ok(tokens_to_sink.upsert(known_tokens))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TokenValidator;
    use crate::data_sources::memory_provider::MemoryProvider;
    use crate::types::{EdgeToken, TokenType, TokenValidationStatus};
    use actix_http::HttpService;
    use actix_http_test::{test_server, TestServer};
    use actix_service::map_config;
    use actix_web::{dev::AppConfig, web, App, HttpResponse};
    use dashmap::DashMap;
    use serde::{Deserialize, Serialize};
    use std::sync::Arc;

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct EdgeTokens {
        pub tokens: Vec<EdgeToken>,
    }

    async fn return_validated_tokens() -> HttpResponse {
        HttpResponse::Ok().json(EdgeTokens {
            tokens: valid_tokens(),
        })
    }

    fn valid_tokens() -> Vec<EdgeToken> {
        vec![EdgeToken {
            token: "*:development.1d38eefdd7bf72676122b008dcf330f2f2aa2f3031438e1b7e8f0d1f".into(),
            projects: vec!["*".into()],
            environment: Some("development".into()),
            token_type: Some(TokenType::Client),
            status: TokenValidationStatus::Validated,
        }]
    }

    async fn test_validation_server() -> TestServer {
        test_server(move || {
            HttpService::new(map_config(
                App::new().service(
                    web::resource("/edge/validate").route(web::post().to(return_validated_tokens)),
                ),
                |_| AppConfig::default(),
            ))
            .tcp()
        })
        .await
    }

    #[tokio::test]
    pub async fn can_validate_tokens() {
        let srv = test_validation_server().await;
        let unleash_client =
            crate::http::unleash_client::UnleashClient::new(srv.url("/").as_str(), None)
                .expect("Couldn't build client");
        let validation_holder = TokenValidator {
            unleash_client: Arc::new(unleash_client),
            token_cache: Arc::new(DashMap::default()),
        };

        let tokens_to_validate = vec![
            "*:development.1d38eefdd7bf72676122b008dcf330f2f2aa2f3031438e1b7e8f0d1f".into(),
            "*:production.abcdef1234567890".into(),
        ];
        validation_holder
            .register_tokens(tokens_to_validate)
            .await
            .expect("Couldn't register tokens");
        assert_eq!(validation_holder.token_cache.len(), 2);
        assert!(validation_holder.token_cache.iter().any(|t| t.value().token
            == "*:development.1d38eefdd7bf72676122b008dcf330f2f2aa2f3031438e1b7e8f0d1f"
            && t.status == TokenValidationStatus::Validated));
        assert!(validation_holder.token_cache.iter()
            .any(|t| t.value().token == "*:production.abcdef1234567890"
                && t.value().status == TokenValidationStatus::Invalid));
    }

    #[tokio::test]
    pub async fn tokens_with_wrong_format_is_not_included() {
        let test_provider = Arc::new(MemoryProvider::default());

        let srv = test_validation_server().await;
        let unleash_client =
            crate::http::unleash_client::UnleashClient::new(srv.url("/").as_str(), None)
                .expect("Couldn't build client");
        let validation_holder = TokenValidator {
            unleash_client: Arc::new(unleash_client),
            token_cache: Arc::new(DashMap::default()),
        };
        let invalid_tokens = vec!["jamesbond".into(), "invalidtoken".into()];
        let validated_tokens = validation_holder.register_tokens(invalid_tokens).await;
        assert!(validated_tokens.is_err());
    }
}
