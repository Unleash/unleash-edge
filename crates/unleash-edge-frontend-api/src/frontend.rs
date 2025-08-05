use axum::extract::{ConnectInfo, Query, State};
use axum::{Json, Router};
use axum::body::Body;
use axum::http::{Response, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use unleash_types::client_features::Context;
use unleash_types::client_metrics::{ClientApplication, ClientMetrics};
use unleash_types::frontend::FrontendResult;
use unleash_edge_appstate::AppState;
use unleash_edge_types::{ClientIp, ClientMetric, EdgeJsonResult};
use unleash_edge_types::tokens::EdgeToken;
use crate::{all_features, enabled_features};

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
pub async fn frontend_get_all_features(app_state: State<AppState>, edge_token: EdgeToken, client_ip: ConnectInfo<ClientIp>, context: Query<Context>) -> EdgeJsonResult<FrontendResult> {
    all_features(app_state.0, edge_token, &context.0, client_ip.ip)
}

pub async fn frontend_post_all_features(app_state: State<AppState>, edge_token: EdgeToken, client_ip: ConnectInfo<ClientIp>, context: Json<Context>)-> EdgeJsonResult<FrontendResult> {
    all_features(app_state.0, edge_token, &context.0, client_ip.ip)
}

pub async fn frontend_get_enabled_features(app_state: State<AppState>, edge_token: EdgeToken, client_ip: ConnectInfo<ClientIp>, context: Query<Context>) -> EdgeJsonResult<FrontendResult> {
    enabled_features(app_state.0, edge_token, &context.0, client_ip.ip)
}

pub async fn frontend_post_enabled_features(app_state: State<AppState>, edge_token: EdgeToken, client_ip: ConnectInfo<ClientIp>, context: Json<Context>) -> EdgeJsonResult<FrontendResult> {
    enabled_features(app_state.0, edge_token, &context.0, client_ip.ip)
}

pub async fn frontend_post_metrics(app_state: State<AppState>, edge_token: EdgeToken, metrics: Json<ClientMetrics>) -> impl IntoResponse {
    unleash_edge_metrics::client_metrics::register_client_metrics(edge_token, metrics.0, app_state.metrics_cache.clone());
    Response::builder().status(StatusCode::ACCEPTED).body(Body::empty()).unwrap()
}

pub async fn frontend_register_client(app_state: State<AppState>, edge_token: EdgeToken, client_application: Json<ClientApplication>) -> impl IntoResponse {
    unleash_edge_metrics::client_metrics::register_client_application(edge_token, &app_state.connect_via, client_application.0, app_state.metrics_cache.clone());
    Response::builder().status(StatusCode::ACCEPTED).body(Body::empty()).unwrap()
}

pub fn router(disable_all_endpoints: bool) -> Router<AppState> {
    let mut common_router = Router::new()
        .route("/frontend", get(frontend_get_enabled_features).post(frontend_post_enabled_features))
        .route("/frontend/client/metrics", post(frontend_post_metrics))
        .route("/frontend/client/register", post(frontend_register_client));
    if !disable_all_endpoints {
        common_router = common_router
            .route("/frontend/all", get(frontend_get_all_features).post(frontend_post_all_features))
            .route("/frontend/all/client/metrics", post(frontend_post_metrics))
            .route("/frontend/all/client/register", post(frontend_register_client));
    }
    common_router
}