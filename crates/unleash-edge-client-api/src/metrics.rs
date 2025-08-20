use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use tracing::instrument;
use unleash_edge_appstate::AppState;
use unleash_edge_metrics::client_metrics::{register_bulk_metrics, register_client_metrics};
use unleash_edge_types::tokens::EdgeToken;
use unleash_edge_types::{AcceptedJson, BatchMetricsRequestBody, EdgeAcceptedJsonResult};
use unleash_types::client_metrics::ClientMetrics;

#[utoipa::path(
    post,
    path = "/metrics",
    context_path = "/api/client",
    responses(
        (status = 202, description = "Accepted client metrics"),
        (status = 403, description = "Was not allowed to post metrics"),
    ),
    request_body = ClientMetrics,
    security(
        ("Authorization" = [])
    )
)]
#[instrument(skip(app_state, edge_token, metrics))]
pub async fn post_metrics(
    app_state: State<AppState>,
    edge_token: EdgeToken,
    metrics: Json<ClientMetrics>,
) -> EdgeAcceptedJsonResult<()> {
    register_client_metrics(edge_token, metrics.0, app_state.metrics_cache.clone());
    Ok(AcceptedJson { body: () })
}

#[utoipa::path(
post,
path = "/bulk",
context_path = "/api/client/metrics",
responses(
(status = 202, description = "Accepted bulk metrics"),
(status = 403, description = "Was not allowed to post bulk metrics")
),
request_body = BatchMetricsRequestBody,
security(
("Authorization" = [])
)
)]
#[instrument(skip(app_state, edge_token, bulk_metrics))]
pub async fn post_bulk_metrics(
    app_state: State<AppState>,
    edge_token: EdgeToken,
    bulk_metrics: Json<BatchMetricsRequestBody>,
) -> EdgeAcceptedJsonResult<()> {
    register_bulk_metrics(
        &app_state.metrics_cache,
        &app_state.connect_via,
        &edge_token,
        bulk_metrics.0,
    );
    Ok(AcceptedJson { body: () })
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/metrics", post(post_metrics))
        .route("/metrics/bulk", post(post_bulk_metrics))
}
