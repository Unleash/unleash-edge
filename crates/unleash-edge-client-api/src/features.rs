use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use unleash_edge_appstate::AppState;
use unleash_edge_feature_filters::{
    filter_client_features, name_prefix_filter, project_filter, FeatureFilterSet,
};
use unleash_edge_types::errors::EdgeError;
use unleash_edge_types::tokens::{cache_key, EdgeToken};
use unleash_edge_types::{EdgeJsonResult, EdgeResult, FeatureFilters, TokenCache};
use unleash_types::client_features::ClientFeatures;

#[utoipa::path(
    get,
    path = "/features",
    context_path = "/api/client",
    params(FeatureFilters),
    responses(
        (status = 200, description = "Return feature toggles for this token", body = ClientFeatures),
        (status = 403, description = "Was not allowed to access features"),
        (status = 400, description = "Invalid parameters used")
    ),
    security(
        ("Authorization" = [])
    )
)]
pub async fn get_features(
    app_state: State<AppState>,
    edge_token: EdgeToken,
    filter_query: Query<FeatureFilters>,
) -> EdgeJsonResult<ClientFeatures> {
    resolve_features(&app_state, edge_token.clone(), filter_query.0.clone()).await
}

#[utoipa::path(
    post,
    path = "/features",
    context_path = "/api/client",
    params(FeatureFilters),
    responses(
        (status = 200, description = "Return feature toggles for this token", body = ClientFeatures),
        (status = 403, description = "Was not allowed to access features"),
        (status = 400, description = "Invalid parameters used")
    ),
    security(
        ("Authorization" = [])
    )
)]
#[axum::debug_handler]
pub async fn post_features(
    app_state: State<AppState>,
    edge_token: EdgeToken,
    filter_query: Query<FeatureFilters>,
) -> EdgeJsonResult<ClientFeatures> {
    resolve_features(&app_state, edge_token, filter_query.0).await
}

async fn resolve_features(
    app_state: &AppState,
    edge_token: EdgeToken,
    filter_query: FeatureFilters,
) -> EdgeJsonResult<ClientFeatures> {
    let (validated_token, filter_set, query) =
        get_feature_filter(&edge_token, &app_state.token_cache, filter_query)?;

    let client_features = match *app_state.feature_refresher {
        Some(ref refresher) => refresher.features_for_filter(validated_token.clone(), &filter_set),
        None => app_state
            .features_cache
            .get(&cache_key(&validated_token))
            .map(|client_features| filter_client_features(&client_features, &filter_set))
            .ok_or(EdgeError::ClientCacheError),
    }?;

    Ok(Json(ClientFeatures {
        query: Some(query),
        ..client_features
    }))
}

fn get_feature_filter(
    edge_token: &EdgeToken,
    token_cache: &TokenCache,
    filter_query: FeatureFilters,
) -> EdgeResult<(
    EdgeToken,
    FeatureFilterSet,
    unleash_types::client_features::Query,
)> {
    let validated_token = token_cache
        .get(&edge_token.token)
        .map(|e| e.value().clone())
        .ok_or(EdgeError::AuthorizationDenied)?;

    let query = unleash_types::client_features::Query {
        tags: None,
        projects: Some(validated_token.projects.clone()),
        name_prefix: filter_query.name_prefix.clone(),
        environment: validated_token.environment.clone(),
        inline_segment_constraints: Some(false),
    };

    let filter_set = if let Some(name_prefix) = filter_query.name_prefix {
        FeatureFilterSet::from(Box::new(name_prefix_filter(name_prefix)))
    } else {
        FeatureFilterSet::default()
    }
    .with_filter(project_filter(&validated_token));

    Ok((validated_token, filter_set, query))
}

pub fn router() -> Router<AppState> {
    Router::new().route("/features", get(get_features).post(post_features))
}
