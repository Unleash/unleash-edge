use axum::extract::{FromRef, Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use dashmap::DashMap;
use std::sync::Arc;
use tracing::{instrument, trace};
use unleash_edge_appstate::AppState;
use unleash_edge_appstate::edge_token_extractor::{AuthState, AuthToken};
use unleash_edge_feature_cache::FeatureCache;
use unleash_edge_feature_filters::{
    FeatureFilterSet, filter_client_features, name_prefix_filter, project_filter,
};
use unleash_edge_feature_refresh::{HydratorType, features_for_filter};
use unleash_edge_types::errors::EdgeError;
use unleash_edge_types::tokens::{EdgeToken, cache_key};
use unleash_edge_types::{EdgeJsonResult, EdgeResult, FeatureFilters, TokenCache, TokenRefresh};
use unleash_types::client_features::{ClientFeature, ClientFeatures};

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
#[instrument(skip(app_state, edge_token, filter_query))]
pub async fn get_features(
    State(app_state): State<FeatureState>,
    AuthToken(edge_token): AuthToken,
    Query(filter_query): Query<FeatureFilters>,
) -> EdgeJsonResult<ClientFeatures> {
    resolve_features(&app_state, edge_token.clone(), filter_query).await
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
#[instrument(skip(app_state, edge_token, filter_query))]
pub async fn post_features(
    State(app_state): State<FeatureState>,
    AuthToken(edge_token): AuthToken,
    Query(filter_query): Query<FeatureFilters>,
) -> EdgeJsonResult<ClientFeatures> {
    resolve_features(&app_state, edge_token, filter_query).await
}

#[utoipa::path(
    get,
    path = "/features/{feature_name}",
    context_path = "/api/client",
    params(
        ("feature_name" = String, Path, description = "The name of the feature")
    ),
    responses(
        (status = 200, description = "Return a single feature toggle for this token", body = ClientFeature),
        (status = 403, description = "Was not allowed to access features"),
        (status = 404, description = "The feature was not found")
    ),
    security(
        ("Authorization" = [])
    )
)]
#[instrument(skip(app_state, edge_token))]
pub async fn get_feature(
    State(app_state): State<FeatureState>,
    AuthToken(edge_token): AuthToken,
    Path(feature_name): Path<String>,
) -> EdgeJsonResult<ClientFeature> {
    let filters = FeatureFilters { name_prefix: None };

    let Json(features) = resolve_features(&app_state, edge_token, filters).await?;
    let feature = features
        .features
        .into_iter()
        .find(|feature| feature.name == feature_name)
        .ok_or(EdgeError::FeatureNotFound(feature_name))?;

    Ok(Json(feature))
}

#[instrument(skip(app_state, edge_token, filter_query))]
async fn resolve_features(
    app_state: &FeatureState,
    edge_token: EdgeToken,
    filter_query: FeatureFilters,
) -> EdgeJsonResult<ClientFeatures> {
    let (validated_token, filter_set, query) =
        get_feature_filter(&edge_token, &app_state.token_cache, filter_query)?;

    let client_features = match &app_state.tokens_to_refresh {
        Some(tokens_to_refresh) => features_for_filter(
            tokens_to_refresh,
            &app_state.features_cache,
            validated_token.clone(),
            &filter_set,
        ),
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

#[instrument(skip(edge_token, token_cache, filter_query))]
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
        .ok_or_else(|| {
            trace!("Could not find token in cache");
            EdgeError::AuthorizationDenied
        })?;

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

#[derive(Clone)]
pub struct FeatureState {
    pub tokens_to_refresh: Option<Arc<DashMap<String, TokenRefresh>>>,
    pub features_cache: Arc<FeatureCache>,
    pub token_cache: Arc<TokenCache>,
}

impl FromRef<AppState> for FeatureState {
    fn from_ref(app: &AppState) -> Self {
        let tokens_to_refresh = match &app.hydrator {
            Some(HydratorType::Streaming(streamer)) => Some(streamer.tokens_to_refresh.clone()),
            Some(HydratorType::Polling(poller)) => Some(poller.tokens_to_refresh.clone()),
            None => None,
        };

        Self {
            features_cache: app.features_cache.clone(),
            token_cache: app.token_cache.clone(),
            tokens_to_refresh,
        }
    }
}

pub fn features_router_for<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
    FeatureState: FromRef<S>,
    AuthState: FromRef<S>,
{
    Router::new()
        .route("/features", get(get_features).post(post_features))
        .route("/features/{feature_name}", get(get_feature))
}

pub fn router() -> Router<AppState> {
    features_router_for::<AppState>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::FromRef;
    use axum::http::StatusCode;
    use axum_test::TestServer;
    use std::str::FromStr;
    use std::sync::Arc;
    use unleash_edge_cli::AuthHeaders;
    use unleash_edge_types::tokens::cache_key;

    #[derive(Clone)]
    struct TestState {
        auth: AuthState,
        features: FeatureState,
    }

    impl FromRef<TestState> for AuthState {
        fn from_ref(state: &TestState) -> Self {
            state.auth.clone()
        }
    }

    impl FromRef<TestState> for FeatureState {
        fn from_ref(state: &TestState) -> Self {
            state.features.clone()
        }
    }

    fn build_server(features_cache: Arc<FeatureCache>, token_cache: Arc<TokenCache>) -> TestServer {
        let app_state = TestState {
            auth: AuthState {
                auth_headers: AuthHeaders::default(),
                token_cache: token_cache.clone(),
            },
            features: FeatureState {
                tokens_to_refresh: None,
                features_cache,
                token_cache,
            },
        };

        let router = features_router_for::<TestState>().with_state(app_state);
        TestServer::builder().http_transport().build(router)
    }

    #[tokio::test]
    async fn single_feature_endpoint_returns_feature() {
        let token_cache = Arc::new(TokenCache::default());
        let features_cache = Arc::new(FeatureCache::default());
        let edge_token =
            EdgeToken::from_str("*:development.somesecret").expect("Could not parse edge token");

        token_cache.insert(edge_token.token.clone(), edge_token.clone());
        features_cache.insert(
            cache_key(&edge_token),
            ClientFeatures {
                version: 2,
                features: vec![ClientFeature {
                    name: "my-feature".to_string(),
                    project: Some("default".to_string()),
                    enabled: true,
                    ..ClientFeature::default()
                }],
                query: None,
                segments: None,
                meta: None,
            },
        );

        let server = build_server(features_cache, token_cache);

        let response = server
            .get("/features/my-feature")
            .add_header("Authorization", edge_token.token)
            .await;

        response.assert_status(StatusCode::OK);
        assert_eq!(response.json::<ClientFeature>().name, "my-feature");
    }

    #[tokio::test]
    async fn single_feature_endpoint_returns_not_found_for_missing_feature() {
        let token_cache = Arc::new(TokenCache::default());
        let features_cache = Arc::new(FeatureCache::default());
        let edge_token =
            EdgeToken::from_str("*:development.somesecret").expect("Could not parse edge token");

        token_cache.insert(edge_token.token.clone(), edge_token.clone());
        features_cache.insert(
            cache_key(&edge_token),
            ClientFeatures {
                version: 2,
                features: vec![ClientFeature {
                    name: "my-feature".to_string(),
                    project: Some("default".to_string()),
                    enabled: true,
                    ..ClientFeature::default()
                }],
                query: None,
                segments: None,
                meta: None,
            },
        );

        let server = build_server(features_cache, token_cache);

        let response = server
            .get("/features/unknown-feature")
            .add_header("Authorization", edge_token.token)
            .await;

        response.assert_status(StatusCode::NOT_FOUND);
    }
}
