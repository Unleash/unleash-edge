use axum::{Router, extract::FromRef};
use unleash_edge_appstate::AppState;
use unleash_edge_appstate::edge_token_extractor::AuthState;

use crate::{
    delta::DeltaState, features::FeatureState, metrics::MetricsState, register::RegisterState,
    streaming::StreamingState,
};

pub mod delta;
pub mod features;
pub mod metrics;
pub mod register;
pub mod streaming;

pub trait ClientApiState: Clone + Send + Sync + 'static {}
impl<S> ClientApiState for S
where
    S: Clone + Send + Sync + 'static,
    FeatureState: FromRef<S>,
    DeltaState: FromRef<S>,
    MetricsState: FromRef<S>,
    AuthState: FromRef<S>,
    RegisterState: FromRef<S>,
    StreamingState: FromRef<S>,
{
}

pub fn router_for<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static + ClientApiState,
    FeatureState: FromRef<S>,
    DeltaState: FromRef<S>,
    MetricsState: FromRef<S>,
    AuthState: FromRef<S>,
    RegisterState: FromRef<S>,
    StreamingState: FromRef<S>,
{
    Router::new()
        .merge(features::features_router_for::<S>())
        .merge(delta::delta_router_for::<S>())
        .merge(metrics::metrics_router_for::<S>())
        .merge(register::register_router_for::<S>())
        .merge(streaming::streaming_router_for::<S>())
}

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(features::router())
        .merge(delta::router())
        .merge(metrics::router())
        .merge(register::router())
        .merge(streaming::router())
}
