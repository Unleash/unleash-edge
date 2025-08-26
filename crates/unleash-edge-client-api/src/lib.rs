use axum::Router;
use tracing::info;
use unleash_edge_appstate::AppState;
use unleash_edge_feature_filters::{FeatureFilterSet, name_prefix_filter, project_filter};
use unleash_edge_types::errors::EdgeError;
use unleash_edge_types::tokens::EdgeToken;
use unleash_edge_types::{EdgeResult, FeatureFilters, TokenCache};

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
