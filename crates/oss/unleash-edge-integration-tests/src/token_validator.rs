#[cfg(test)]
mod tests {
    use axum::extract::FromRef;
    use axum::response::IntoResponse;
    use axum::routing::post;
    use axum::{Json, Router};
    use axum_test::TestServer;
    use chrono::Duration;
    use dashmap::DashMap;
    use reqwest::Url;
    use serde::{Deserialize, Serialize};
    use std::sync::Arc;
    use ulid::Ulid;
    use unleash_edge_auth::token_validator::TokenValidator;
    use unleash_edge_edge_api::{EdgeApiState, edge_api_router_for};
    use unleash_edge_http_client::{
        ClientMetaInformation, HttpClientArgs, UnleashClient, new_reqwest_client,
    };
    use unleash_edge_types::tokens::EdgeToken;
    use unleash_edge_types::{TokenCache, TokenType, TokenValidationStatus};

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct EdgeTokens {
        pub tokens: Vec<EdgeToken>,
    }

    trait TestConfig {
        fn test_config() -> Self;
    }

    impl TestConfig for ClientMetaInformation {
        fn test_config() -> Self {
            ClientMetaInformation {
                app_name: "test_app".into(),
                instance_id: Ulid::new(),
                connection_id: Ulid::new(),
            }
        }
    }

    #[derive(Clone)]
    pub struct TestState {
        pub token_cache: Arc<TokenCache>,
        pub token_validator: Arc<Option<TokenValidator>>,
    }

    impl FromRef<TestState> for EdgeApiState {
        fn from_ref(app_state: &TestState) -> Self {
            EdgeApiState {
                token_cache: app_state.token_cache.clone(),
                token_validator: app_state.token_validator.clone(),
            }
        }
    }

    pub fn build_unleash_client(server_url: Url) -> Arc<UnleashClient> {
        Arc::new(UnleashClient::from_url_with_backing_client(
            server_url,
            "Authorization".to_string(),
            new_reqwest_client(HttpClientArgs {
                skip_ssl_verification: false,
                client_identity: None,
                upstream_certificate_file: None,
                connect_timeout: Duration::seconds(10),
                socket_timeout: Duration::seconds(10),
                keep_alive_timeout: Duration::seconds(10),
                client_meta_information: ClientMetaInformation::test_config(),
            })
            .unwrap(),
            ClientMetaInformation::test_config(),
        ))
    }

    async fn return_validated_tokens() -> impl IntoResponse {
        let tokens = EdgeTokens {
            tokens: valid_tokens().clone(),
        };
        Json(tokens)
    }

    fn valid_tokens() -> Vec<EdgeToken> {
        vec![EdgeToken {
            token: "*:development.1d38eefdd7bf72676122b008dcf330f2f2aa2f3031438e1b7e8f0d1f".into(),
            projects: vec!["*".into()],
            environment: Some("development".into()),
            token_type: Some(TokenType::Backend),
            status: TokenValidationStatus::Validated,
        }]
    }

    async fn test_validation_server() -> TestServer {
        let router = Router::new().route("/edge/validate", post(return_validated_tokens));
        TestServer::builder()
            .http_transport()
            .build(router)
            .unwrap()
    }

    async fn validation_server_with_valid_tokens(
        token_cache: Arc<DashMap<String, EdgeToken>>,
    ) -> TestServer {
        let token_validator = TokenValidator {
            token_cache: token_cache.clone(),
            persistence: None,
            unleash_client: build_unleash_client(Url::parse("http://localhost:4242").unwrap()),
            deferred_validation_tx: None,
        };
        let test_state = TestState {
            token_cache,
            token_validator: Arc::new(Some(token_validator)),
        };

        let router = Router::new()
            .nest("/edge", edge_api_router_for::<TestState>())
            .with_state(test_state);
        TestServer::builder()
            .http_transport()
            .build(router)
            .unwrap()
    }

    #[tokio::test]
    pub async fn can_validate_tokens() {
        let srv = test_validation_server().await;
        let unleash_client =
            build_unleash_client(srv.server_url("/").expect("Couldn't build client"));
        let validation_holder = TokenValidator {
            unleash_client,
            token_cache: Arc::new(DashMap::default()),
            persistence: None,
            deferred_validation_tx: None,
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
            build_unleash_client(srv.server_url("/").expect("Couldn't build client"));
        let validation_holder = TokenValidator {
            unleash_client,
            token_cache: Arc::new(DashMap::default()),
            persistence: None,
            deferred_validation_tx: None,
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
        valid_token_development.status = TokenValidationStatus::Validated;
        valid_token_development.token_type = Some(TokenType::Backend);
        upstream_tokens.insert(
            valid_token_development.token.clone(),
            valid_token_development.clone(),
        );
        let mut no_longer_valid_token = EdgeToken::try_from("*:production.123secret".to_string())
            .expect("Bad test production token");
        no_longer_valid_token.status = TokenValidationStatus::Invalid;
        no_longer_valid_token.token_type = Some(TokenType::Backend);
        upstream_tokens.insert(
            no_longer_valid_token.token.clone(),
            no_longer_valid_token.clone(),
        );

        let srv = validation_server_with_valid_tokens(upstream_tokens).await;
        let unleash_client =
            build_unleash_client(srv.server_url("/").expect("Couldn't build client"));

        let local_token_cache = Arc::new(DashMap::default());
        let mut previously_valid_token = no_longer_valid_token.clone();
        previously_valid_token.status = TokenValidationStatus::Validated;
        local_token_cache.insert(
            previously_valid_token.token.clone(),
            previously_valid_token.clone(),
        );
        let validation_holder = TokenValidator {
            unleash_client,
            token_cache: local_token_cache.clone(),
            persistence: None,
            deferred_validation_tx: None,
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
        valid_token_development.status = TokenValidationStatus::Validated;
        valid_token_development.token_type = Some(TokenType::Backend);
        let mut valid_token_production =
            EdgeToken::try_from("*:production.magic123".to_string()).expect("Bad Test Data");
        valid_token_production.status = TokenValidationStatus::Validated;
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
        let unleash_client =
            build_unleash_client(server.server_url("/").expect("Couldn't build client"));
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
            unleash_client,
            persistence: None,
            deferred_validation_tx: None,
        };
        let _ = validator.revalidate_known_tokens().await;
        assert_eq!(validator.token_cache.len(), 2);
        assert!(
            validator
                .token_cache
                .iter()
                .all(|t| t.value().status == TokenValidationStatus::Validated)
        );
    }

    #[tokio::test]
    pub async fn deferred_validation_sends_tokens_to_channel() {
        let upstream_tokens: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());
        let mut valid_token_development =
            EdgeToken::try_from("*:development.secret123".to_string()).expect("Bad Test Data");
        valid_token_development.status = TokenValidationStatus::Validated;
        valid_token_development.token_type = Some(TokenType::Backend);
        upstream_tokens.insert(
            valid_token_development.token.clone(),
            valid_token_development.clone(),
        );

        let server = validation_server_with_valid_tokens(upstream_tokens).await;
        let unleash_client =
            build_unleash_client(server.server_url("/").expect("Couldn't build client"));
        let local_tokens: DashMap<String, EdgeToken> = DashMap::default();
        local_tokens.insert(
            valid_token_development.token.clone(),
            valid_token_development.clone(),
        );

        let (deferred_validation_tx, mut deferred_validation_rx) =
            tokio::sync::mpsc::unbounded_channel();
        let validator = TokenValidator {
            token_cache: Arc::new(local_tokens),
            unleash_client,
            persistence: None,
            deferred_validation_tx: Some(deferred_validation_tx),
        };
        let token = EdgeToken {
            token: "*:development.token".into(),
            projects: vec!["*".into()],
            environment: Some("test".into()),
            token_type: Some(TokenType::Backend),
            status: TokenValidationStatus::Unknown,
        };

        validator
            .deferred_token_registration(vec![token.token.clone()])
            .expect("Couldn't register token");

        assert!(deferred_validation_rx.recv().await.is_some());
    }
}
