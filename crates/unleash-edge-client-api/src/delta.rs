use axum::body::Body;
use axum::extract::{FromRef, FromRequestParts, Query, State};
use axum::http::request::Parts;
use axum::http::{Response, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router, http};
use std::sync::Arc;
use tracing::instrument;
use unleash_edge_appstate::AppState;
use unleash_edge_appstate::edge_token_extractor::{AuthState, AuthToken};
use unleash_edge_feature_filters::delta_filters::{DeltaFilterSet, combined_filter};
use unleash_edge_feature_filters::get_feature_filter;
use unleash_edge_feature_refresh::HydratorType;
use unleash_edge_feature_refresh::delta_refresh::DeltaRefresher;
use unleash_edge_types::errors::EdgeError;
use unleash_edge_types::tokens::EdgeToken;
use unleash_edge_types::{EdgeJsonResult, EdgeResult, FeatureFilters, TokenCache};
use unleash_types::client_features::ClientFeaturesDelta;

#[derive(Debug, Clone, Default)]
pub struct RevisionId {
    requested_revision_id: u32,
}

impl<S> FromRequestParts<S> for RevisionId
where
    S: Send + Sync,
{
    type Rejection = EdgeError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(parts
            .headers
            .get(http::header::IF_NONE_MATCH)
            .and_then(|value| value.to_str().ok())
            .and_then(|etag| etag.trim_matches('"').parse::<u32>().ok())
            .map(|r| RevisionId {
                requested_revision_id: r,
            })
            .unwrap_or_default())
    }
}

pub struct DeltaResolverArgs {
    pub edge_token: EdgeToken,
    pub token_cache: Arc<TokenCache>,
    pub filter_query: FeatureFilters,
    pub delta_refresher: Arc<DeltaRefresher>,
    pub requested_revision_id: u32,
}

#[instrument(skip(app_state, edge_token, filter_query, revision_id))]
pub async fn get_features_delta(
    app_state: State<DeltaState>,
    AuthToken(edge_token): AuthToken,
    revision_id: RevisionId,
    Query(filter_query): Query<FeatureFilters>,
) -> impl IntoResponse {
    match app_state.hydrator.clone() {
        Some(HydratorType::Polling(_)) => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("Cannot resolve delta in polling mode"))
            .unwrap(),
        Some(HydratorType::Streaming(ref delta_refresher)) => {
            match resolve_delta(DeltaResolverArgs {
                edge_token,
                token_cache: app_state.token_cache.clone(),
                filter_query,
                delta_refresher: delta_refresher.clone(),
                requested_revision_id: revision_id.requested_revision_id,
            })
            .await
            {
                Ok(Json(None)) => Response::builder()
                    .status(StatusCode::NOT_MODIFIED)
                    .body(Body::empty())
                    .unwrap(),
                Ok(Json(Some(delta))) => {
                    let last_event_id = delta.events.last().map(|e| e.get_event_id()).unwrap_or(0);
                    Response::builder()
                        .status(StatusCode::OK)
                        .header(http::header::ETAG, format!("{}", last_event_id))
                        .body(Body::from(serde_json::to_string(&delta).unwrap()))
                        .unwrap()
                }
                Err(e) => Response::builder()
                    .status(e.status_code())
                    .body(Body::empty())
                    .unwrap(),
            }
        }
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("Cannot resolve delta in offline mode"))
            .unwrap(),
    }
}

#[instrument(skip(edge_token, token_cache, query_filters, requested_revision_id))]
fn get_delta_filter(
    edge_token: &EdgeToken,
    token_cache: &TokenCache,
    query_filters: FeatureFilters,
    requested_revision_id: u32,
) -> EdgeResult<DeltaFilterSet> {
    let validated_token = token_cache
        .get(&edge_token.token)
        .map(|e| e.value().clone())
        .ok_or(EdgeError::AuthorizationDenied)?;

    let delta_filter_set = DeltaFilterSet::default().with_filter(combined_filter(
        requested_revision_id,
        validated_token.projects.clone(),
        query_filters.name_prefix.clone(),
    ));

    Ok(delta_filter_set)
}

#[instrument(skip(args))]
async fn resolve_delta(args: DeltaResolverArgs) -> EdgeJsonResult<Option<ClientFeaturesDelta>> {
    let (validated_token, filter_set, ..) = get_feature_filter(
        &args.edge_token,
        &args.token_cache,
        args.filter_query.clone(),
    )?;

    let delta_filter_set = get_delta_filter(
        &args.edge_token,
        &args.token_cache,
        args.filter_query.clone(),
        args.requested_revision_id,
    )?;

    let delta = args.delta_refresher.delta_events_for_filter(
        validated_token.clone(),
        filter_set,
        delta_filter_set,
        args.requested_revision_id,
    )?;

    if delta.events.is_empty() {
        return Ok(Json(None));
    }

    Ok(Json(Some(delta)))
}

#[derive(Clone)]
pub struct DeltaState {
    pub hydrator: Option<HydratorType>,
    pub token_cache: Arc<TokenCache>,
}

impl FromRef<AppState> for DeltaState {
    fn from_ref(app: &AppState) -> Self {
        Self {
            hydrator: app.hydrator.clone(),
            token_cache: app.token_cache.clone(),
        }
    }
}

fn features_router_for<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
    DeltaState: FromRef<S>,
    AuthState: FromRef<S>,
{
    Router::new().route("/delta", get(get_features_delta))
}

pub fn router() -> Router<AppState> {
    features_router_for::<AppState>()
}
