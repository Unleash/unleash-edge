use axum::extract::{ConnectInfo, Query, State};
use axum::{Json, Router};
use axum::body::Body;
use axum::http::{Response, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use unleash_types::client_features::Context;
use unleash_types::client_metrics::{ClientApplication, ClientMetrics, ConnectVia};
use unleash_types::frontend::FrontendResult;
use unleash_edge_appstate::AppState;
use unleash_edge_types::{ClientIp, EdgeJsonResult};
use unleash_edge_types::tokens::EdgeToken;
use crate::{all_features, enabled_features};

#[utoipa::path(
get,
path = "/all",
context_path = "/api/proxy",
responses(
(status = 200, description = "Return all known feature toggles for this token in evaluated (true|false) state", body = FrontendResult),
(status = 403, description = "Was not allowed to access features"),
(status = 400, description = "Bad data in query parameters")
),
request_body = Context,
security(
("Authorization" = [])
)
)]
pub async fn get_proxy_all_features(app_state: State<AppState>, edge_token: EdgeToken, connect_info: ConnectInfo<ClientIp>, context: Query<Context>) -> EdgeJsonResult<FrontendResult> {
    all_features(app_state.0, edge_token, &context.0, connect_info.ip)
}

#[utoipa::path(
post,
path = "/all",
context_path = "/api/proxy",
responses(
(status = 200, description = "Return all known feature toggles for this token in evaluated (true|false) state", body = FrontendResult),
(status = 403, description = "Was not allowed to access features"),
(status = 400, description = "Invalid parameters used")
),
request_body = Context,
security(
("Authorization" = [])
)
)]
#[axum::debug_handler]
pub async fn post_proxy_all_features(app_state: State<AppState>, edge_token: EdgeToken, connect_info: ConnectInfo<ClientIp>, context: Json<Context>) -> EdgeJsonResult<FrontendResult> {
    all_features(app_state.0, edge_token, &context.0, connect_info.ip)
}

pub async fn proxy_post_metrics(app_state: State<AppState>, edge_token: EdgeToken, metrics: Json<ClientMetrics>) -> impl IntoResponse {
    unleash_edge_metrics::client_metrics::register_client_metrics(edge_token, metrics.0, app_state.metrics_cache.clone());
    Response::builder().status(StatusCode::ACCEPTED).body(Body::empty()).unwrap()
}

pub async fn proxy_register_client(app_state: State<AppState>, edge_token: EdgeToken, client_application: Json<ClientApplication>) -> impl IntoResponse {
    unleash_edge_metrics::client_metrics::register_client_application(edge_token, &app_state.connect_via, client_application.0, app_state.metrics_cache.clone());
    Response::builder().status(StatusCode::ACCEPTED).body(Body::empty()).unwrap()
}

pub async fn proxy_get_enabled_features(app_state: State<AppState>, edge_token: EdgeToken, client_ip: ConnectInfo<ClientIp>, context: Query<Context>) -> EdgeJsonResult<FrontendResult> {
    enabled_features(app_state.0, edge_token, &context.0, client_ip.ip)
}

pub async fn proxy_post_enabled_features(app_state: State<AppState>, edge_token: EdgeToken, client_ip: ConnectInfo<ClientIp>, context: Json<Context>) -> EdgeJsonResult<FrontendResult> {
    enabled_features(app_state.0, edge_token, &context.0, client_ip.ip)
}

pub fn router(disable_all_endpoint: bool) -> Router<AppState> {
    let mut base_router = Router::new()
        .route("/proxy", get(proxy_get_enabled_features).post(proxy_post_enabled_features))
        .route("/proxy/client/register", post(proxy_register_client))
        .route("/proxy/client/metrics", post(proxy_post_metrics));
    if !disable_all_endpoint {
        base_router = base_router
            .route("/proxy/all", get(get_proxy_all_features).post(post_proxy_all_features))
            .route("/proxy/all/client/register", post(proxy_register_client))
            .route("/proxy/all/client/metrics", post(proxy_post_metrics))
    }
    base_router
}