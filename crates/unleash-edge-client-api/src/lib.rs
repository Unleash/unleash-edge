use axum::Router;
use unleash_edge_appstate::AppState;

pub mod delta;
pub mod features;
pub mod metrics;
pub mod register;
pub mod streaming;

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(features::router())
        .merge(delta::router())
        .merge(metrics::router())
        .merge(register::router())
        .merge(streaming::router())
}
