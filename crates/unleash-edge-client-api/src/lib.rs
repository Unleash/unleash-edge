use axum::extract::{FromRequestParts, Query};
use axum::http::request::Parts;
use axum::Router;
use unleash_types::client_metrics::ConnectVia;
use unleash_edge_appstate::AppState;
use unleash_edge_feature_filters::{name_prefix_filter, project_filter, FeatureFilterSet};
use unleash_edge_types::errors::EdgeError;
use unleash_edge_types::{EdgeResult, FeatureFilters, TokenCache};
use unleash_edge_types::tokens::EdgeToken;

pub mod features;
pub mod delta;
pub mod metrics;
pub mod register;

fn get_feature_filter(
    edge_token: &EdgeToken,
    token_cache: &TokenCache,
    filter_query: Query<FeatureFilters>,
) -> EdgeResult<(
    EdgeToken,
    FeatureFilterSet,
    unleash_types::client_features::Query,
)> {
    let validated_token = token_cache
        .get(&edge_token.token)
        .map(|e| e.value().clone())
        .ok_or(EdgeError::AuthorizationDenied)?;

    let query_filters = filter_query.0;
    let query = unleash_types::client_features::Query {
        tags: None,
        projects: Some(validated_token.projects.clone()),
        name_prefix: query_filters.name_prefix.clone(),
        environment: validated_token.environment.clone(),
        inline_segment_constraints: Some(false),
    };

    let filter_set = if let Some(name_prefix) = query_filters.name_prefix {
        FeatureFilterSet::from(Box::new(name_prefix_filter(name_prefix)))
    } else {
        FeatureFilterSet::default()
    }
        .with_filter(project_filter(&validated_token));

    Ok((validated_token, filter_set, query))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .nest("/features", features::router())
        .nest("/delta", delta::router())
        .nest("/metrics", metrics::router())
        .nest("/register", register::router())
}