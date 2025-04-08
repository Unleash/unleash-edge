use std::sync::Arc;

use dashmap::DashMap;
use tracing::trace;
use unleash_types::Upsert;

use crate::http::refresher::feature_refresher::FeatureRefresher;
use crate::http::unleash_client::UnleashClient;
use crate::persistence::EdgePersistence;
use crate::types::{
    EdgeResult, EdgeToken, TokenType, TokenValidationStatus, ValidateTokensRequest,
};

#[derive(Clone)]
pub struct TokenValidator {
    pub unleash_client: Arc<UnleashClient>,
    pub token_cache: Arc<DashMap<String, EdgeToken>>,
    pub persistence: Option<Arc<dyn EdgePersistence>>,
}

impl TokenValidator {
    async fn get_unknown_and_known_tokens(
        &self,
        tokens: Vec<String>,
    ) -> (Vec<EdgeToken>, Vec<EdgeToken>) {
        let tokens_with_valid_format: Vec<EdgeToken> = tokens
            .into_iter()
            .filter_map(|t| EdgeToken::try_from(t).ok())
            .collect();

        if tokens_with_valid_format.is_empty() {
            (vec![], vec![])
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
            tokens.into_iter().partition(|t| t.token_type.is_none())
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
        let (unknown_tokens, known_tokens) = self.get_unknown_and_known_tokens(tokens).await;
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
                        trace!("Validated token");
                        validated_token.clone()
                    } else {
                        trace!("Invalid token");
                        EdgeToken {
                            status: TokenValidationStatus::Invalid,
                            token_type: Some(TokenType::Invalid),
                            ..maybe_valid
                        }
                    }
                })
                .collect();
            tokens_to_sink.iter().for_each(|t| {
                self.token_cache.insert(t.token.clone(), t.clone());
            });
            let updated_tokens = tokens_to_sink.upsert(known_tokens);
            if let Some(persist) = self.persistence.clone() {
                let _ = persist.save_tokens(updated_tokens.clone()).await;
            }
            Ok(updated_tokens)
        }
    }

    pub async fn schedule_validation_of_known_tokens(&self, validation_interval_seconds: u64) {
        let sleep_duration = tokio::time::Duration::from_secs(validation_interval_seconds);
        loop {
            tokio::select! {
                _ = tokio::time::sleep(sleep_duration) => {
                    let _ = self.revalidate_known_tokens().await;
                }
            }
        }
    }

    pub async fn schedule_revalidation_of_startup_tokens(
        &self,
        tokens: Vec<String>,
        refresher: Option<Arc<FeatureRefresher>>,
    ) {
        let sleep_duration = tokio::time::Duration::from_secs(1);
        loop {
            tokio::select! {
                _ = tokio::time::sleep(sleep_duration) => {
                    if let Some(refresher) = refresher.clone() {
                        let token_result = self.register_tokens(tokens.clone()).await;
                        if let Ok(good_tokens) = token_result {
                            for token in good_tokens {
                                let _ = refresher.register_and_hydrate_token(&token).await;
                            }
                        }
                    }
                }
            }
        }
    }

    pub async fn revalidate_known_tokens(&self) -> EdgeResult<()> {
        let tokens_to_validate: Vec<String> = self
            .token_cache
            .iter()
            .filter(|t| t.value().is_known())
            .map(|e| e.key().clone())
            .collect();
        if !tokens_to_validate.is_empty() {
            let validation_result = self
                .unleash_client
                .validate_tokens(ValidateTokensRequest {
                    tokens: tokens_to_validate.clone(),
                })
                .await;

            if let Ok(valid_tokens) = validation_result {
                let invalid = tokens_to_validate
                    .into_iter()
                    .filter(|t| !valid_tokens.iter().any(|e| &e.token == t));
                for token in invalid {
                    self.token_cache
                        .entry(token)
                        .and_modify(|t| t.status = TokenValidationStatus::Invalid);
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use actix_http::HttpService;
    use actix_http_test::{TestServer, test_server};
    use actix_service::map_config;
    use actix_web::{App, HttpResponse, dev::AppConfig, web};
    use dashmap::DashMap;
    use serde::{Deserialize, Serialize};

    use super::TokenValidator;
    use crate::types::{Environment, Projects};
    use crate::{
        http::unleash_client::UnleashClient,
        types::{EdgeToken, TokenType, TokenValidationStatus},
    };

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
            token_type: Some(TokenType::Client),
            status: TokenValidationStatus::Validated(
                Projects::new(&vec!["*".into()]),
                Environment::new("development".into()),
            ),
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

    async fn validation_server_with_valid_tokens(
        token_cache: Arc<DashMap<String, EdgeToken>>,
    ) -> TestServer {
        let token_cache_wrapper = web::Data::from(token_cache.clone());
        let token_validator = web::Data::new(TokenValidator {
            token_cache: token_cache.clone(),
            persistence: None,
            unleash_client: Arc::new(UnleashClient::new("http://localhost:4242", None).unwrap()),
        });
        test_server(move || {
            HttpService::new(map_config(
                App::new()
                    .app_data(token_cache_wrapper.clone())
                    .app_data(token_validator.clone())
                    .service(web::scope("/edge").service(crate::edge_api::validate)),
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
            persistence: None,
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
            && t.status
                == TokenValidationStatus::Validated(
                    Projects::new(&vec!["*".into()]),
                    Environment::new("development")
                )));
        assert!(
            validation_holder
                .token_cache
                .iter()
                .any(|t| t.value().token == "*:production.abcdef1234567890"
                    && t.value().status == TokenValidationStatus::Invalid)
        );
    }

    #[tokio::test]
    pub async fn tokens_with_wrong_format_is_not_included() {
        let srv = test_validation_server().await;
        let unleash_client =
            UnleashClient::new(srv.url("/").as_str(), None).expect("Couldn't build client");
        let validation_holder = TokenValidator {
            unleash_client: Arc::new(unleash_client),
            token_cache: Arc::new(DashMap::default()),
            persistence: None,
        };
        let invalid_tokens = vec!["jamesbond".into(), "invalidtoken".into()];
        let validated_tokens = validation_holder
            .register_tokens(invalid_tokens)
            .await
            .unwrap();
        assert!(validated_tokens.is_empty());
    }

    #[tokio::test]
    pub async fn upstream_invalid_tokens_are_set_to_invalid() {
        let upstream_tokens = Arc::new(DashMap::default());
        let mut valid_token_development =
            EdgeToken::try_from("*:development.secret123".to_string()).expect("Bad Test Data");
        valid_token_development.status = TokenValidationStatus::Validated(
            Projects::new(&["*".into()]),
            Environment::new("development"),
        );
        valid_token_development.token_type = Some(TokenType::Client);
        upstream_tokens.insert(
            valid_token_development.token.clone(),
            valid_token_development.clone(),
        );
        let mut no_longer_valid_token = EdgeToken::try_from("*:production.123secret".to_string())
            .expect("Bad test production token");
        no_longer_valid_token.status = TokenValidationStatus::Invalid;
        no_longer_valid_token.token_type = Some(TokenType::Client);
        upstream_tokens.insert(
            no_longer_valid_token.token.clone(),
            no_longer_valid_token.clone(),
        );

        let srv = validation_server_with_valid_tokens(upstream_tokens).await;
        let unleash_client =
            crate::http::unleash_client::UnleashClient::new(srv.url("/").as_str(), None)
                .expect("Couldn't build client");

        let local_token_cache = Arc::new(DashMap::default());
        let mut previously_valid_token = no_longer_valid_token.clone();
        previously_valid_token.status = TokenValidationStatus::Validated(
            Projects::wildcard_project(),
            Environment::new("production"),
        );
        local_token_cache.insert(
            previously_valid_token.token.clone(),
            previously_valid_token.clone(),
        );
        let validation_holder = TokenValidator {
            unleash_client: Arc::new(unleash_client),
            token_cache: local_token_cache.clone(),
            persistence: None,
        };
        let _ = validation_holder.revalidate_known_tokens().await;
        assert!(
            validation_holder
                .token_cache
                .iter()
                .all(|t| t.value().status == TokenValidationStatus::Invalid)
        );
    }

    #[tokio::test]
    pub async fn still_valid_tokens_are_left_untouched() {
        let upstream_tokens: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());
        let mut valid_token_development =
            EdgeToken::try_from("*:development.secret123".to_string()).expect("Bad Test Data");
        valid_token_development.status = TokenValidationStatus::Validated(
            Projects::wildcard_project(),
            Environment::new("development"),
        );
        valid_token_development.token_type = Some(TokenType::Client);
        let mut valid_token_production =
            EdgeToken::try_from("*:production.magic123".to_string()).expect("Bad Test Data");
        valid_token_production.status = TokenValidationStatus::Validated(
            Projects::wildcard_project(),
            Environment::new("production"),
        );
        valid_token_production.token_type = Some(TokenType::Frontend);
        upstream_tokens.insert(
            valid_token_development.token.clone(),
            valid_token_development.clone(),
        );
        upstream_tokens.insert(
            valid_token_production.token.clone(),
            valid_token_production.clone(),
        );
        let server = validation_server_with_valid_tokens(upstream_tokens).await;
        let client = UnleashClient::new(server.url("/").as_str(), None).unwrap();
        let local_tokens: DashMap<String, EdgeToken> = DashMap::default();
        local_tokens.insert(
            valid_token_development.token.clone(),
            valid_token_development,
        );
        local_tokens.insert(
            valid_token_production.token.clone(),
            valid_token_production.clone(),
        );
        let validator = TokenValidator {
            token_cache: Arc::new(local_tokens),
            unleash_client: Arc::new(client),
            persistence: None,
        };
        let _ = validator.revalidate_known_tokens().await;
        assert_eq!(validator.token_cache.len(), 2);
    }
}
