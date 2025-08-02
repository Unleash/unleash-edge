use std::sync::Arc;
use axum::extract::{Query, Request, State};
use axum::response::IntoResponse;
use axum::{http, Json, Router};
use axum::body::Body;
use axum::http::{HeaderMap, Response, StatusCode};
use axum::routing::get;
use tracing::instrument;
use unleash_types::client_features::ClientFeaturesDelta;
use unleash_edge_appstate::AppState;
use unleash_edge_feature_filters::delta_filters::{combined_filter, DeltaFilterSet};
use unleash_edge_feature_refresh::FeatureRefresher;
use unleash_edge_types::{EdgeJsonResult, EdgeResult, FeatureFilters, TokenCache};
use unleash_edge_types::errors::EdgeError;
use unleash_edge_types::tokens::EdgeToken;
use crate::get_feature_filter;

pub struct DeltaResolverArgs {
    pub edge_token: EdgeToken,
    pub token_cache: Arc<TokenCache>,
    pub filter_query: Query<FeatureFilters>,
    pub features_refresher: Arc<Option<FeatureRefresher>>,
    pub requested_revision_id: u32,
}
#[instrument(skip(app_state, headers, edge_token, filter_query))]
#[axum::debug_handler]
pub async fn get_features_delta(app_state: State<AppState>, headers: HeaderMap, edge_token: EdgeToken, filter_query: Query<FeatureFilters>) -> impl IntoResponse {
    let requested_revision_id = headers.get(http::header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .and_then(|etag| etag.trim_matches('"').parse::<u32>().ok())
        .unwrap_or(0);
    Response::builder().status(StatusCode::OK).body(Body::empty()).unwrap()
    /*match resolve_delta(
        DeltaResolverArgs {
            edge_token,
            token_cache: app_state.token_cache.clone(),
            filter_query,
            features_refresher: app_state.feature_refresher.clone(),
            requested_revision_id,
        }
    ).await {
        Ok(Json(None)) => Response::builder().status(StatusCode::NOT_MODIFIED).body(Body::empty()).unwrap(),
        Ok(Json(Some(delta))) => {
            let last_event_id = delta.events.last().map(|e| e.get_event_id()).unwrap_or(0);
            Response::builder().status(StatusCode::OK).header(axum::http::header::ETAG, format!("{}", last_event_id)).body(Body::from(serde_json::to_string(&delta).unwrap())).unwrap()
        }
        Err(e) => Response::builder().status(e.status_code()).body(Body::empty()).unwrap(),
    }*/
}

fn get_delta_filter(
    edge_token: &EdgeToken,
    token_cache: &TokenCache,
    filter_query: Query<FeatureFilters>,
    requested_revision_id: u32,
) -> EdgeResult<DeltaFilterSet> {
    let validated_token = token_cache
        .get(&edge_token.token)
        .map(|e| e.value().clone())
        .ok_or(EdgeError::AuthorizationDenied)?;

    let query_filters = filter_query.0;

    let delta_filter_set = DeltaFilterSet::default().with_filter(combined_filter(
        requested_revision_id,
        validated_token.projects.clone(),
        query_filters.name_prefix.clone(),
    ));

    Ok(delta_filter_set)
}

async fn resolve_delta(args: DeltaResolverArgs) -> EdgeJsonResult<Option<ClientFeaturesDelta>> {
    let (validated_token, filter_set, ..) =
        get_feature_filter(&args.edge_token, &args.token_cache, args.filter_query.clone())?;

    let delta_filter_set = get_delta_filter(
        &args.edge_token,
        &args.token_cache,
        args.filter_query.clone(),
        args.requested_revision_id,
    )?;

    match *args.features_refresher {
        Some(ref refresher) => {
            let delta = refresher
                .delta_events_for_filter(
                    validated_token.clone(),
                    &filter_set,
                    &delta_filter_set,
                    args.requested_revision_id,
                )
                .await?;

            if delta.events.is_empty() {
                return Ok(Json(None));
            }

            Ok(Json(Some(delta)))
        }
        None => Err(EdgeError::ClientHydrationFailed(
            "FeatureRefresher is missing - cannot resolve delta in offline mode".to_string(),
        ))
    }
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/features", get(get_features_delta))
}