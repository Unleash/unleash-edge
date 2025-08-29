use crate::querystring_extractor::QsQueryCfg;
use crate::{all_features, enabled_features};
use axum::body::Body;
use axum::extract::{ConnectInfo, Path, State};
use axum::http::{Response, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use std::net::{IpAddr, SocketAddr};
use tracing::instrument;
use unleash_edge_appstate::AppState;
use unleash_edge_types::errors::EdgeError;
use unleash_edge_types::tokens::{EdgeToken, cache_key};
use unleash_edge_types::{EdgeJsonResult, EdgeResult, EngineCache, TokenCache};
use unleash_types::client_features::Context;
use unleash_types::client_metrics::{ClientApplication, ClientMetrics};
use unleash_types::frontend::{EvaluatedToggle, EvaluatedVariant, FrontendResult};

#[utoipa::path(
    get,
    path = "/all",
    context_path = "/api/frontend",
    responses(
(status = 200, description = "Return all known feature toggles for this token in evaluated (true|false) state", body = FrontendResult),
(status = 403, description = "Was not allowed to access features")
    ),
    params(Context),
    security(
("Authorization" = [])
    )
)]
#[instrument(skip(app_state, edge_token, client_ip, context))]
pub async fn frontend_get_all_features(
    State(app_state): State<AppState>,
    edge_token: EdgeToken,
    client_ip: ConnectInfo<SocketAddr>,
    QsQueryCfg(context): QsQueryCfg<Context>,
) -> EdgeJsonResult<FrontendResult> {
    all_features(app_state, edge_token, &context, client_ip.ip())
}

#[instrument(skip(app_state, edge_token, client_ip, context))]
pub async fn frontend_post_all_features(
    State(app_state): State<AppState>,
    edge_token: EdgeToken,
    client_ip: ConnectInfo<SocketAddr>,
    Json(context): Json<Context>,
) -> EdgeJsonResult<FrontendResult> {
    all_features(app_state, edge_token, &context, client_ip.ip())
}

#[instrument(skip(app_state, edge_token, client_ip, context))]
pub async fn frontend_get_enabled_features(
    State(app_state): State<AppState>,
    edge_token: EdgeToken,
    client_ip: ConnectInfo<SocketAddr>,
    QsQueryCfg(context): QsQueryCfg<Context>,
) -> EdgeJsonResult<FrontendResult> {
    enabled_features(app_state, edge_token, &context, client_ip.ip())
}

#[instrument(skip(app_state, edge_token, client_ip, context))]
pub async fn frontend_post_enabled_features(
    State(app_state): State<AppState>,
    edge_token: EdgeToken,
    client_ip: ConnectInfo<SocketAddr>,
    Json(context): Json<Context>,
) -> EdgeJsonResult<FrontendResult> {
    enabled_features(app_state, edge_token, &context, client_ip.ip())
}

#[instrument(skip(app_state, edge_token, metrics))]
pub async fn frontend_post_metrics(
    app_state: State<AppState>,
    edge_token: EdgeToken,
    Json(metrics): Json<ClientMetrics>,
) -> impl IntoResponse {
    unleash_edge_metrics::client_metrics::register_client_metrics(
        edge_token,
        metrics,
        app_state.metrics_cache.clone(),
    );
    Response::builder()
        .status(StatusCode::ACCEPTED)
        .body(Body::empty())
        .unwrap()
}

#[instrument(skip(app_state, edge_token, client_application))]
pub async fn frontend_register_client(
    State(app_state): State<AppState>,
    edge_token: EdgeToken,
    Json(client_application): Json<ClientApplication>,
) -> impl IntoResponse {
    unleash_edge_metrics::client_metrics::register_client_application(
        edge_token,
        &app_state.connect_via,
        client_application,
        app_state.metrics_cache.clone(),
    );
    Response::builder()
        .status(StatusCode::ACCEPTED)
        .body(Body::empty())
        .unwrap()
}

#[instrument(skip(app_state, edge_token, feature_name, context, connect_info))]
pub async fn frontend_get_feature(
    State(app_state): State<AppState>,
    edge_token: EdgeToken,
    Path(feature_name): Path<String>,
    QsQueryCfg(context): QsQueryCfg<Context>,
    ConnectInfo(connect_info): ConnectInfo<SocketAddr>,
) -> EdgeJsonResult<EvaluatedToggle> {
    evaluate_feature(
        &app_state.token_cache,
        &app_state.engine_cache,
        &edge_token,
        feature_name,
        context,
        connect_info.ip(),
    )
    .map(Json)
}

#[instrument(skip(app_state, edge_token, feature_name, context, connect_info))]
pub async fn frontend_post_feature(
    State(app_state): State<AppState>,
    edge_token: EdgeToken,
    Path(feature_name): Path<String>,
    ConnectInfo(connect_info): ConnectInfo<SocketAddr>,
    Json(context): Json<Context>,
) -> EdgeJsonResult<EvaluatedToggle> {
    evaluate_feature(
        &app_state.token_cache,
        &app_state.engine_cache,
        &edge_token,
        feature_name,
        context,
        connect_info.ip(),
    )
    .map(Json)
}

#[instrument(skip(token_cache, engine_cache, edge_token, feature_name, incoming_context))]
fn evaluate_feature(
    token_cache: &TokenCache,
    engine_cache: &EngineCache,
    edge_token: &EdgeToken,
    feature_name: String,
    incoming_context: Context,
    ip: IpAddr,
) -> EdgeResult<EvaluatedToggle> {
    let context: Context = incoming_context.clone();
    let context_with_ip = if context.remote_address.is_none() {
        Context {
            remote_address: Some(ip.to_string()),
            ..context
        }
    } else {
        context
    };
    let validated_token = token_cache
        .get(&edge_token.token)
        .ok_or(EdgeError::EdgeTokenError)?
        .value()
        .clone();
    engine_cache
        .get(&cache_key(&validated_token))
        .and_then(|engine| engine.resolve(&feature_name, &context_with_ip, &None))
        .and_then(|resolved_toggle| {
            if validated_token.projects.contains(&"*".into())
                || validated_token.projects.contains(&resolved_toggle.project)
            {
                Some(resolved_toggle)
            } else {
                None
            }
        })
        .map(|r| EvaluatedToggle {
            name: feature_name.clone(),
            enabled: r.enabled,
            variant: EvaluatedVariant {
                name: r.variant.name,
                enabled: r.variant.enabled,
                payload: r.variant.payload,
            },
            impression_data: r.impression_data,
            impressionData: r.impression_data,
        })
        .ok_or_else(|| EdgeError::FeatureNotFound(feature_name.clone()))
}

pub fn router(disable_all_endpoints: bool) -> Router<AppState> {
    let mut common_router = Router::new()
        .route(
            "/frontend",
            get(frontend_get_enabled_features).post(frontend_post_enabled_features),
        )
        .route(
            "/frontend/features/{feature_name}",
            get(frontend_get_feature).post(frontend_post_feature),
        )
        .route("/frontend/client/metrics", post(frontend_post_metrics))
        .route("/frontend/client/register", post(frontend_register_client))
        .route(
            "/proxy",
            get(frontend_get_enabled_features).post(frontend_post_enabled_features),
        )
        .route("/proxy/client/metrics", post(frontend_post_metrics))
        .route("/proxy/client/register", post(frontend_register_client));
    if !disable_all_endpoints {
        common_router = common_router
            .route(
                "/frontend/all",
                get(frontend_get_all_features).post(frontend_post_all_features),
            )
            .route("/frontend/all/client/metrics", post(frontend_post_metrics))
            .route(
                "/frontend/all/client/register",
                post(frontend_register_client),
            )
            .route(
                "/proxy/all",
                get(frontend_get_all_features).post(frontend_post_all_features),
            )
            .route("/proxy/all/client/metrics", post(frontend_post_metrics))
            .route("/proxy/all/client/register", post(frontend_register_client));
    }
    common_router
}

#[cfg(test)]
mod tests {
    use axum::extract::connect_info::MockConnectInfo;
    use axum::http::StatusCode;
    use axum_test::TestServer;
    use serde_json::json;
    use std::fs;
    use std::io::BufReader;
    use std::net::SocketAddr;
    use std::path::PathBuf;
    use std::str::FromStr;
    use std::sync::Arc;
    use unleash_edge_appstate::AppState;
    use unleash_edge_types::tokens::{EdgeToken, cache_key};
    use unleash_edge_types::{EngineCache, TokenCache, TokenType, TokenValidationStatus};
    use unleash_types::client_features::{
        ClientFeature, ClientFeatures, Constraint, Operator, Strategy,
    };
    use unleash_types::frontend::FrontendResult;
    use unleash_yggdrasil::{EngineState, UpdateMessage};

    fn frontend_test_server(app_state: AppState, disable_all_endpoints: bool) -> TestServer {
        let router = super::router(disable_all_endpoints)
            .with_state(app_state)
            .into_make_service_with_connect_info::<SocketAddr>();
        TestServer::builder()
            .http_transport()
            .build(router)
            .expect("Failed to build test server")
    }

    fn frontend_test_server_with_ip(app_state: AppState, ip_addr: &str) -> TestServer {
        let fake_addr = SocketAddr::from_str(ip_addr).unwrap();
        let router = super::router(false)
            .with_state(app_state)
            .layer(MockConnectInfo(fake_addr));
        TestServer::builder()
            .http_transport()
            .build(router)
            .expect("Failed to build test server")
    }

    #[tokio::test]
    async fn get_requests_to_enabled_endpoint_gets_only_enabled_features() {
        let token_cache = Arc::new(TokenCache::new());
        let mut frontend_token =
            EdgeToken::from_str("*:development.abc123").expect("Failed to parse frontend token");
        frontend_token.token_type = Some(TokenType::Frontend);
        frontend_token.status = TokenValidationStatus::Validated;
        let mut engine_state = EngineState::default();
        engine_state.take_state(UpdateMessage::FullResponse(
            client_features_with_one_enabled_toggle_and_one_disabled_toggle(),
        ));
        let engine_cache = Arc::new(EngineCache::new());
        engine_cache.insert(cache_key(&frontend_token), engine_state);
        let app_state = AppState::builder()
            .with_token_cache(token_cache)
            .with_engine_cache(engine_cache)
            .build();
        let server = frontend_test_server(app_state.clone(), true);
        let res = server
            .get("/frontend")
            .add_header("Authorization", frontend_token.token)
            .await;
        assert_eq!(res.status_code(), StatusCode::OK);
        let result = res.json::<FrontendResult>();
        assert_eq!(result.toggles.len(), 1);
        assert_eq!(result.toggles[0].name, "test");
    }

    #[tokio::test]
    async fn get_requests_to_all_endpoint_gets_all_features() {
        let token_cache = Arc::new(TokenCache::new());
        let mut frontend_token =
            EdgeToken::from_str("*:development.abc123").expect("Failed to parse frontend token");
        frontend_token.token_type = Some(TokenType::Frontend);
        frontend_token.status = TokenValidationStatus::Validated;
        let mut engine_state = EngineState::default();
        engine_state.take_state(UpdateMessage::FullResponse(
            client_features_with_one_enabled_toggle_and_one_disabled_toggle(),
        ));
        let engine_cache = Arc::new(EngineCache::new());
        engine_cache.insert(cache_key(&frontend_token), engine_state);
        let app_state = AppState::builder()
            .with_token_cache(token_cache)
            .with_engine_cache(engine_cache)
            .build();
        let server = frontend_test_server(app_state.clone(), false);
        let res = server
            .get("/frontend/all")
            .add_header("Authorization", frontend_token.token)
            .await;
        assert_eq!(res.status_code(), StatusCode::OK);
        let result = res.json::<FrontendResult>();
        assert_eq!(result.toggles.len(), 2);
        assert!(result.toggles[0].enabled);
        assert!(!result.toggles[1].enabled);
    }

    #[tokio::test]
    async fn get_requests_parses_context_from_url() {
        let token_cache = Arc::new(TokenCache::new());
        let mut frontend_token =
            EdgeToken::from_str("*:development.abc123").expect("Failed to parse frontend token");
        frontend_token.token_type = Some(TokenType::Frontend);
        frontend_token.status = TokenValidationStatus::Validated;
        let mut engine_state = EngineState::default();
        engine_state.take_state(UpdateMessage::FullResponse(
            client_features_with_constraint_requiring_user_id_of_seven(),
        ));
        let engine_cache = Arc::new(EngineCache::new());
        engine_cache.insert(cache_key(&frontend_token), engine_state);
        let app_state = AppState::builder()
            .with_token_cache(token_cache)
            .with_engine_cache(engine_cache)
            .build();
        let server = frontend_test_server(app_state.clone(), true);
        let res = server
            .get("/frontend?userId=7")
            .add_header("Authorization", frontend_token.token.clone())
            .await;
        assert_eq!(res.status_code(), StatusCode::OK);
        let result = res.json::<FrontendResult>();
        assert_eq!(result.toggles.len(), 1);
        assert_eq!(result.toggles[0].name, "test");
        let wrong_user_response = server
            .get("/frontend?userId=152")
            .add_header("Authorization", frontend_token.token)
            .await;
        assert_eq!(wrong_user_response.status_code(), StatusCode::OK);
        let wrong_user_result = wrong_user_response.json::<FrontendResult>();
        assert!(wrong_user_result.toggles.is_empty());
    }

    #[tokio::test]
    async fn post_requests_parses_context_from_body() {
        let token_cache = Arc::new(TokenCache::new());
        let mut frontend_token =
            EdgeToken::from_str("*:development.abc123").expect("Failed to parse frontend token");
        frontend_token.token_type = Some(TokenType::Frontend);
        frontend_token.status = TokenValidationStatus::Validated;
        let mut engine_state = EngineState::default();
        engine_state.take_state(UpdateMessage::FullResponse(
            client_features_with_constraint_requiring_user_id_of_seven(),
        ));
        let engine_cache = Arc::new(EngineCache::new());
        engine_cache.insert(cache_key(&frontend_token), engine_state);
        let app_state = AppState::builder()
            .with_token_cache(token_cache)
            .with_engine_cache(engine_cache)
            .build();
        let server = frontend_test_server(app_state.clone(), true);
        let res = server
            .post("/frontend")
            .add_header("Authorization", frontend_token.token.clone())
            .json(&json!({
                "userId": 7
            }))
            .await;
        assert_eq!(res.status_code(), StatusCode::OK);
        let result = res.json::<FrontendResult>();
        assert_eq!(result.toggles.len(), 1);
        assert_eq!(result.toggles[0].name, "test");
        let wrong_user_response = server
            .post("/frontend")
            .add_header("Authorization", frontend_token.token)
            .json(&json!({
                "userId": 170
            }))
            .await;
        assert_eq!(wrong_user_response.status_code(), StatusCode::OK);
        let wrong_user_result = wrong_user_response.json::<FrontendResult>();
        assert!(wrong_user_result.toggles.is_empty());
    }

    #[tokio::test]
    async fn proxy_and_frontend_returns_same_response() {
        let token_cache = Arc::new(TokenCache::new());
        let mut frontend_token =
            EdgeToken::from_str("*:development.abc123").expect("Failed to parse frontend token");
        frontend_token.token_type = Some(TokenType::Frontend);
        frontend_token.status = TokenValidationStatus::Validated;
        let mut engine_state = EngineState::default();
        engine_state.take_state(UpdateMessage::FullResponse(
            client_features_with_one_enabled_toggle_and_one_disabled_toggle(),
        ));
        let engine_cache = Arc::new(EngineCache::new());
        engine_cache.insert(cache_key(&frontend_token), engine_state);
        let app_state = AppState::builder()
            .with_token_cache(token_cache)
            .with_engine_cache(engine_cache)
            .build();
        let server = frontend_test_server(app_state.clone(), true);
        let frontend_response = server
            .get("/frontend")
            .add_header("Authorization", frontend_token.token.clone())
            .await;
        assert_eq!(frontend_response.status_code(), StatusCode::OK);
        let frontend_result = frontend_response.json::<FrontendResult>();
        let proxy_response = server
            .get("/proxy")
            .add_header("Authorization", frontend_token.token)
            .await;
        assert_eq!(proxy_response.status_code(), StatusCode::OK);
        let proxy_result = proxy_response.json::<FrontendResult>();
        assert!(frontend_result.toggles.iter().all(|frontend_toggle| {
            proxy_result.toggles.iter().any(|proxy_toggle| {
                frontend_toggle.enabled == proxy_toggle.enabled
                    && frontend_toggle.impression_data == proxy_toggle.impression_data
                    && frontend_toggle.name == proxy_toggle.name
            })
        }))
    }

    #[tokio::test]
    async fn can_get_single_feature_with_top_level_properties() {
        let token_cache = Arc::new(TokenCache::new());
        let mut frontend_token =
            EdgeToken::from_str("*:development.abc123").expect("Failed to parse frontend token");
        frontend_token.token_type = Some(TokenType::Frontend);
        frontend_token.status = TokenValidationStatus::Validated;
        token_cache.insert(frontend_token.token.clone(), frontend_token.clone());
        let mut engine_state = EngineState::default();
        engine_state.take_state(UpdateMessage::FullResponse(
            client_features_with_constraint_requiring_test_property_to_be_42(),
        ));
        let engine_cache = Arc::new(EngineCache::new());
        engine_cache.insert(cache_key(&frontend_token), engine_state);
        let app_state = AppState::builder()
            .with_token_cache(token_cache)
            .with_engine_cache(engine_cache)
            .build();
        let server = frontend_test_server(app_state.clone(), true);
        let res = server
            .get("/frontend/features/test?test_property=42")
            .add_header("Authorization", frontend_token.token.clone())
            .await;
        assert_eq!(res.status_code(), StatusCode::OK);
    }

    #[tokio::test]
    async fn trying_to_evaluate_feature_you_do_not_have_access_to_will_give_not_found() {
        let token_cache = Arc::new(TokenCache::new());
        let mut frontend_token =
            EdgeToken::from_str("dx:development.abc123").expect("Failed to parse frontend token");
        frontend_token.token_type = Some(TokenType::Frontend);
        frontend_token.status = TokenValidationStatus::Validated;
        token_cache.insert(frontend_token.token.clone(), frontend_token.clone());
        let mut engine_state = EngineState::default();
        engine_state.take_state(UpdateMessage::FullResponse(features_from_disk(
            "../../examples/hostedexample.json",
        )));
        let engine_cache = Arc::new(EngineCache::new());
        engine_cache.insert(cache_key(&frontend_token), engine_state);
        let app_state = AppState::builder()
            .with_token_cache(token_cache)
            .with_engine_cache(engine_cache)
            .build();
        let server = frontend_test_server(app_state.clone(), true);
        let res = server
            .get("/frontend/features/variantsPerEnvironment")
            .add_header("Authorization", frontend_token.token.clone())
            .await;
        assert_eq!(res.status_code(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn can_handle_custom_context_fields() {
        let token_cache = Arc::new(TokenCache::new());
        let mut frontend_token =
            EdgeToken::from_str("*:development.abc123").expect("Failed to parse frontend token");
        frontend_token.token_type = Some(TokenType::Frontend);
        frontend_token.status = TokenValidationStatus::Validated;
        token_cache.insert(frontend_token.token.clone(), frontend_token.clone());
        let mut engine_state = EngineState::default();
        engine_state.take_state(UpdateMessage::FullResponse(features_from_disk(
            "../../examples/with_custom_constraint.json",
        )));
        let engine_cache = Arc::new(EngineCache::new());
        engine_cache.insert(cache_key(&frontend_token), engine_state);
        let app_state = AppState::builder()
            .with_token_cache(token_cache)
            .with_engine_cache(engine_cache)
            .build();
        let server = frontend_test_server(app_state.clone(), true);
        let res = server
            .get("/frontend?properties[companyId]=bricks")
            .add_header("Authorization", frontend_token.token.clone())
            .await;
        assert_eq!(res.status_code(), StatusCode::OK);
        let frontend_result = res.json::<FrontendResult>();
        assert_eq!(frontend_result.toggles.len(), 1);
    }
    #[tokio::test]
    async fn can_handle_custom_context_fields_with_post() {
        let token_cache = Arc::new(TokenCache::new());
        let mut frontend_token =
            EdgeToken::from_str("*:development.abc123").expect("Failed to parse frontend token");
        frontend_token.token_type = Some(TokenType::Frontend);
        frontend_token.status = TokenValidationStatus::Validated;
        token_cache.insert(frontend_token.token.clone(), frontend_token.clone());
        let mut engine_state = EngineState::default();
        engine_state.take_state(UpdateMessage::FullResponse(features_from_disk(
            "../../examples/with_custom_constraint.json",
        )));
        let engine_cache = Arc::new(EngineCache::new());
        engine_cache.insert(cache_key(&frontend_token), engine_state);
        let app_state = AppState::builder()
            .with_token_cache(token_cache)
            .with_engine_cache(engine_cache)
            .build();
        let server = frontend_test_server(app_state.clone(), true);
        let res = server
            .post("/frontend")
            .add_header("Authorization", frontend_token.token.clone())
            .json(&json!({
                "properties": {
                    "companyId": "bricks"
                }
            }))
            .await;
        assert_eq!(res.status_code(), StatusCode::OK);
        let frontend_result = res.json::<FrontendResult>();
        assert_eq!(frontend_result.toggles.len(), 1);
    }

    #[tokio::test]
    async fn will_evaluate_ip_strategy_from_middleware() {
        let token_cache = Arc::new(TokenCache::new());
        let mut frontend_token =
            EdgeToken::from_str("*:development.abc123").expect("Failed to parse frontend token");
        frontend_token.token_type = Some(TokenType::Frontend);
        frontend_token.status = TokenValidationStatus::Validated;
        token_cache.insert(frontend_token.token.clone(), frontend_token.clone());
        let mut engine_state = EngineState::default();
        engine_state.take_state(UpdateMessage::FullResponse(features_from_disk(
            "../../examples/ip_address_feature.json",
        )));
        let engine_cache = Arc::new(EngineCache::new());
        engine_cache.insert(cache_key(&frontend_token), engine_state);
        let app_state = AppState::builder()
            .with_token_cache(token_cache)
            .with_engine_cache(engine_cache)
            .build();
        let server = frontend_test_server_with_ip(app_state.clone(), "192.168.0.1:80");
        let res = server
            .get("/frontend")
            .add_header("Content-Type", "application/json")
            .add_header("Authorization", frontend_token.token.clone())
            .await;
        assert_eq!(res.status_code(), StatusCode::OK);
        let frontend_result = res.json::<FrontendResult>();
        assert_eq!(frontend_result.toggles.len(), 1);
        assert_eq!(frontend_result.toggles[0].name, "ip_addr");
    }

    fn client_features_with_one_enabled_toggle_and_one_disabled_toggle() -> ClientFeatures {
        ClientFeatures {
            version: 1,
            features: vec![
                ClientFeature {
                    name: "test".into(),
                    enabled: true,
                    strategies: None,
                    ..ClientFeature::default()
                },
                ClientFeature {
                    name: "test2".into(),
                    enabled: false,
                    strategies: None,
                    ..ClientFeature::default()
                },
            ],
            segments: None,
            query: None,
            meta: None,
        }
    }
    fn client_features_with_constraint_requiring_user_id_of_seven() -> ClientFeatures {
        ClientFeatures {
            version: 1,
            features: vec![ClientFeature {
                name: "test".into(),
                enabled: true,
                strategies: Some(vec![Strategy {
                    name: "default".into(),
                    sort_order: None,
                    segments: None,
                    variants: None,
                    constraints: Some(vec![Constraint {
                        context_name: "userId".into(),
                        operator: Operator::In,
                        case_insensitive: false,
                        inverted: false,
                        values: Some(vec!["7".into()]),
                        value: None,
                    }]),
                    parameters: None,
                }]),
                ..ClientFeature::default()
            }],
            segments: None,
            query: None,
            meta: None,
        }
    }
    fn client_features_with_constraint_requiring_test_property_to_be_42() -> ClientFeatures {
        ClientFeatures {
            version: 1,
            features: vec![ClientFeature {
                name: "test".into(),
                enabled: true,
                strategies: Some(vec![Strategy {
                    name: "default".into(),
                    sort_order: None,
                    segments: None,
                    variants: None,
                    constraints: Some(vec![Constraint {
                        context_name: "test_property".into(),
                        operator: Operator::In,
                        case_insensitive: false,
                        inverted: false,
                        values: Some(vec!["42".into()]),
                        value: None,
                    }]),
                    parameters: None,
                }]),
                ..ClientFeature::default()
            }],
            segments: None,
            query: None,
            meta: None,
        }
    }
    fn features_from_disk(path: &str) -> ClientFeatures {
        let path = PathBuf::from(path);
        let file = fs::File::open(path).unwrap();
        let reader = BufReader::new(file);
        serde_json::from_reader(reader).unwrap()
    }
}
