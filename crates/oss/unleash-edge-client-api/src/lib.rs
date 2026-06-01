use axum::{Router, extract::FromRef};
use unleash_edge_appstate::AppState;
use unleash_edge_appstate::edge_token_extractor::AuthState;
use utoipa::openapi::security::{ApiKey, ApiKeyValue, SecurityScheme};
use utoipa::{Modify, OpenApi};

use crate::{
    delta::DeltaState, features::FeatureState, metrics::MetricsState, register::RegisterState,
    streaming::StreamingState,
};

pub mod delta;
pub mod features;
pub mod metrics;
pub mod register;
pub mod streaming;

#[derive(OpenApi)]
#[openapi(
    paths(
        crate::features::get_features,
        crate::features::post_features,
        crate::features::get_feature,
        crate::metrics::post_metrics,
        crate::metrics::post_bulk_metrics,
        crate::register::register
    ),
    tags(
        (name = "Client API", description = "Unleash Edge client endpoints")
    ),
    modifiers(&SecurityAddon)
)]
pub struct ClientApiDoc;

struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi
            .components
            .get_or_insert_with(utoipa::openapi::Components::new);

        components.add_security_scheme(
            "Authorization",
            SecurityScheme::ApiKey(ApiKey::Header(ApiKeyValue::new("Authorization"))),
        );

	// muck with the pathing a little, this gives us a nice user friendly display name,
	// and allows us to stop the library tagging on the crate name which is just annoying noise
        for path_item in openapi.paths.paths.values_mut() {
            if let Some(operation) = path_item.get.as_mut() {
                operation.tags = Some(vec!["Client API".to_string()]);
            }
            if let Some(operation) = path_item.post.as_mut() {
                operation.tags = Some(vec!["Client API".to_string()]);
            }
            if let Some(operation) = path_item.put.as_mut() {
                operation.tags = Some(vec!["Client API".to_string()]);
            }
            if let Some(operation) = path_item.delete.as_mut() {
                operation.tags = Some(vec!["Client API".to_string()]);
            }
            if let Some(operation) = path_item.patch.as_mut() {
                operation.tags = Some(vec!["Client API".to_string()]);
            }
            if let Some(operation) = path_item.options.as_mut() {
                operation.tags = Some(vec!["Client API".to_string()]);
            }
            if let Some(operation) = path_item.head.as_mut() {
                operation.tags = Some(vec!["Client API".to_string()]);
            }
            if let Some(operation) = path_item.trace.as_mut() {
                operation.tags = Some(vec!["Client API".to_string()]);
            }
        }
    }
}

pub fn openapi() -> utoipa::openapi::OpenApi {
    ClientApiDoc::openapi()
}

pub fn router_for<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
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
