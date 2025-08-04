use axum::body::Body;
use axum::extract::State;
use axum::http::{Response, StatusCode};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use tracing::instrument;
use unleash_edge_appstate::AppState;
use unleash_edge_types::EDGE_VERSION;
use unleash_edge_types::tokens::EdgeToken;
use unleash_types::client_metrics::ClientApplication;

#[utoipa::path(
    path = "/register",
    post,
    context_path = "/api/client",
    responses(
        (status = 202, description = "Accepted client application registration"),
        (status = 403, description = "Was not allowed to register client application"),
    ),
    request_body = ClientApplication,
    security(
        ("Authorization" = [])
    )
)]
#[instrument(skip(app_state, edge_token, client_application))]
pub async fn register(
    app_state: State<AppState>,
    edge_token: EdgeToken,
    client_application: Json<ClientApplication>,
) -> impl IntoResponse {
    unleash_edge_metrics::client_metrics::register_client_application(
        edge_token,
        &app_state.connect_via,
        client_application.0,
        app_state.metrics_cache.clone(),
    );
    Response::builder()
        .status(StatusCode::ACCEPTED)
        .header("X-Edge-Version", EDGE_VERSION)
        .body(Body::empty())
        .unwrap()
}

pub fn router() -> Router<AppState> {
    Router::new().route("/register", post(register))
}
