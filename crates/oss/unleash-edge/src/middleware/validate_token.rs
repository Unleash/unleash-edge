use axum::body::Body;
use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use reqwest::StatusCode;
use tracing::{debug, instrument, trace};
use unleash_edge_appstate::AppState;
use unleash_edge_appstate::edge_token_extractor::AuthToken;
use unleash_edge_auth::token_validator::{TokenRegister, TokenValidator};
use unleash_edge_metrics::client_metrics::register_seen_token;
use unleash_edge_types::errors::EdgeError;
use unleash_edge_types::tokens::EdgeToken;
use unleash_edge_types::{TokenType, TokenValidationStatus};

#[instrument(skip(app_state, edge_token, req, next))]
pub async fn validate_token(
    app_state: State<AppState>,
    AuthToken(edge_token): AuthToken,
    req: Request,
    next: Next,
) -> impl IntoResponse {
    let path = req.uri().path();
    // Pretrusted tokens use local authorization even when an upstream validator exists.
    let validator = (*app_state.token_validator)
        .clone()
        .filter(|_| edge_token.status != TokenValidationStatus::Trusted);
    let validation_status = match validator {
        Some(ref validator) => validate_with_validator(&edge_token, path, validator).await,
        None => validate_without_validator(&app_state, &edge_token, path),
    };
    match validation_status {
        Ok(_) => {
            // Locally trusted credentials have no upstream token usage to report.
            if edge_token.status != TokenValidationStatus::Trusted {
                register_seen_token(&app_state.metrics_cache, &edge_token);
            }
            next.run(req).await
        }
        Err(err) => match err {
            EdgeError::AuthorizationDenied => Response::builder()
                .status(StatusCode::UNAUTHORIZED)
                .body(Body::empty())
                .unwrap(),
            EdgeError::Forbidden(_) => Response::builder()
                .status(StatusCode::FORBIDDEN)
                .body(Body::empty())
                .unwrap(),
            _ => err.into_response(),
        },
    }
}

fn validate_without_validator(
    app_state: &State<AppState>,
    edge_token: &EdgeToken,
    path: &str,
) -> Result<(), EdgeError> {
    let token = if edge_token.status == TokenValidationStatus::Trusted {
        // The extractor already resolved the trusted entry using the original secret.
        Some(edge_token.clone())
    } else {
        app_state
            .token_cache
            .get(&edge_token.token)
            .map(|token| token.value().clone())
    };
    match token {
        Some(token) => match token.token_type {
            Some(TokenType::Frontend) => check_frontend_path(path),
            Some(TokenType::Backend) => check_backend_path(path),
            None => Ok(()),
            _ => Err(EdgeError::Forbidden("Unknown token type".into())),
        },
        None => Err(EdgeError::Forbidden("No access allowed for token".into())),
    }
}

#[instrument(skip(edge_token, validator))]
async fn validate_with_validator(
    edge_token: &EdgeToken,
    path: &str,
    validator: &TokenValidator,
) -> Result<(), EdgeError> {
    let known_token = validator.register_token(edge_token.token.clone()).await?;
    match known_token.status {
        TokenValidationStatus::Validated => match known_token.token_type {
            Some(TokenType::Frontend) => check_frontend_path(path),
            Some(TokenType::Backend) => check_backend_path(path),
            None => Ok(()),
            _ => Err(EdgeError::Forbidden("No token type found".into())),
        },
        TokenValidationStatus::Unknown => {
            debug!("Validation status of token was unknown");
            Err(EdgeError::AuthorizationDenied)
        }
        TokenValidationStatus::Invalid => Err(EdgeError::Forbidden(
            "Token validation status was invalid".into(),
        )),
        TokenValidationStatus::Trusted => unreachable!(),
    }
}

fn check_frontend_path(path: &str) -> Result<(), EdgeError> {
    if path.contains("/frontend") || path.contains("/proxy") {
        Ok(())
    } else {
        trace!("Frontend: Denied, when looking at path [{path}]");
        Err(EdgeError::Forbidden(format!(
            "Frontend Token was not allowed to access {path}"
        )))
    }
}

fn check_backend_path(path: &str) -> Result<(), EdgeError> {
    if path.contains("client/") {
        Ok(())
    } else {
        trace!("Backend: Denied, when looking at path [{path}]");
        Err(EdgeError::Forbidden(format!(
            "Backend Token was not allowed to access {path}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::middleware::from_fn_with_state;
    use axum_test::TestServer;
    use std::sync::Arc;
    use tokio::sync::RwLock;
    use unleash_edge_appstate::ContextEnricher;
    use unleash_edge_cli::AuthHeaders;
    use unleash_edge_feature_cache::FeatureCache;
    use unleash_edge_http_client::UnleashClient;
    use unleash_edge_types::enterprise::{ApplicationLicenseState, LicenseState};
    use unleash_edge_types::metrics::MetricsCache;
    use unleash_edge_types::metrics::instance_data::EdgeInstanceData;
    use unleash_edge_types::tokens::{cache_key, parse_trusted_token_pair};
    use unleash_edge_types::{EngineCache, TokenCache};
    use unleash_types::client_features::{ClientFeature, ClientFeatures};
    use unleash_types::client_metrics::ConnectVia;
    use unleash_yggdrasil::{EngineState, UpdateMessage};

    #[tokio::test]
    async fn pretrusted_legacy_secret_authorizes_frontend_and_proxy() {
        for with_validator in [false, true] {
            let token_cache = Arc::new(TokenCache::new());
            // Mirror startup insertion: the key is the secret, not secret@environment.
            let (secret, token) = parse_trusted_token_pair("test@development").unwrap();
            assert_eq!(secret, "test");
            token_cache.insert(secret, token.clone());
            let (secret, pretrusted_token) =
                parse_trusted_token_pair("*:development.pretrusted").unwrap();
            token_cache.insert(secret, pretrusted_token);
            // Keep the existing cached-token paths covered alongside pretrusted tokens.
            for (secret, token_type) in [
                ("*:development.frontend", TokenType::Frontend),
                ("*:development.backend", TokenType::Backend),
            ] {
                let mut cached_token: EdgeToken = secret.parse().unwrap();
                cached_token.token_type = Some(token_type);
                cached_token.status = TokenValidationStatus::Validated;
                token_cache.insert(secret.into(), cached_token);
            }
            let engine_cache = Arc::new(EngineCache::new());
            let engine = EngineState::default();
            engine.take_state(UpdateMessage::FullResponse(ClientFeatures {
                features: vec![ClientFeature {
                    name: "existing-feature".into(),
                    project: Some("default".into()),
                    enabled: false,
                    ..Default::default()
                }],
                ..Default::default()
            }));
            engine_cache.insert(cache_key(&token), engine);
            let upstream = Arc::new(UnleashClient::from_url_with_backing_client(
                "http://127.0.0.1:1".parse().unwrap(),
                "Authorization".into(),
                reqwest::Client::new(),
                unleash_edge_http_client::ClientMetaInformation {
                    app_name: "migration-check".into(),
                    instance_id: ulid::Ulid::generate(),
                    connection_id: ulid::Ulid::generate(),
                },
            ));
            let app = AppState {
                token_validator: Arc::new(
                    with_validator
                        .then(|| TokenValidator::new(upstream, token_cache.clone(), None)),
                ),
                token_cache,
                engine_cache,
                features_cache: Arc::new(FeatureCache::new(Default::default())),
                metrics_cache: Arc::new(MetricsCache::default()),
                hydrator: None,
                delta_cache_manager: None,
                auth_headers: AuthHeaders::default(),
                connect_via: ConnectVia {
                    app_name: "migration-check".into(),
                    instance_id: "test".into(),
                },
                edge_instance_data: Arc::new(EdgeInstanceData::new(
                    "migration-check",
                    &ulid::Ulid::generate(),
                    None,
                )),
                connected_instances: Arc::new(RwLock::new(vec![])),
                deny_list: vec![],
                allow_list: vec![],
                trust_proxy: false,
                proxy_trusted_servers: vec![],
                context_enricher: ContextEnricher::disabled(),
                license_state: ApplicationLicenseState::new(LicenseState::Valid),
            };
            let metrics_cache = app.metrics_cache.clone();
            let router = Router::new()
                .nest(
                    "/api",
                    unleash_edge_frontend_api::router(false)
                        .route(
                            "/client/features",
                            axum::routing::get(|| async { "backend" }),
                        )
                        .layer(from_fn_with_state(app.clone(), validate_token)),
                )
                .with_state(app);
            let server = TestServer::new(router);
            server
                .get("/api/client/features")
                .add_header("Authorization", "*:development.backend")
                .await
                .assert_status_ok();
            server
                .get("/api/client/features")
                .add_header("Authorization", "test")
                .await
                .assert_status_forbidden();
            for path in ["/api/frontend", "/api/proxy"] {
                server
                    .get(path)
                    .add_header("Authorization", "*:development.frontend")
                    .await
                    .assert_status_ok();
                server
                    .get(path)
                    .add_header("Authorization", "*:development.backend")
                    .await
                    .assert_status_forbidden();
                let response = server
                    .get(path)
                    .add_header("Authorization", "test")
                    .add_header("Unleash-Appname", "migration-check")
                    .await;
                response.assert_status_ok();
                response.assert_json(&serde_json::json!({"toggles": []}));
                server
                    .get(path)
                    .add_header("Authorization", "test@development")
                    .await
                    .assert_status_forbidden();
                server
                    .get(path)
                    .add_header("Authorization", "unconfigured-secret")
                    .await
                    .assert_status_forbidden();
            }
            // Single-feature evaluation requires a lookup by the extracted token's identity.
            for request in [
                server.get("/api/frontend/features/existing-feature"),
                server
                    .post("/api/frontend/features/existing-feature")
                    .json(&serde_json::json!({})),
            ] {
                let response = request
                    .add_header("Authorization", "test")
                    .add_header("Unleash-Appname", "migration-check")
                    .await;
                response.assert_status_ok();
                let toggle = response.json::<serde_json::Value>();
                assert_eq!(toggle["name"], "existing-feature");
                assert_eq!(toggle["enabled"], false);
            }
            server
                .get("/api/frontend")
                .add_header("Authorization", "*:development.pretrusted")
                .await
                .assert_status_ok();
            // Only ordinary Unleash tokens should be reported upstream as seen.
            assert_eq!(metrics_cache.seen_tokens.len(), 2);
            for secret in ["*:development.frontend", "*:development.backend"] {
                assert!(
                    metrics_cache
                        .seen_tokens
                        .contains(&secret.parse::<EdgeToken>().unwrap())
                );
            }
        }
    }
}
