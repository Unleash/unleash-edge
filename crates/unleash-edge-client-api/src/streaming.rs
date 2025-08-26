use std::{convert::Infallible, time::Duration};

use axum::{
    Router,
    extract::{Query, State},
    response::{Sse, sse::Event},
    routing::get,
};
use futures_util::{Stream, StreamExt};
use tokio_stream::once;
use tokio_stream::wrappers::BroadcastStream;
use unleash_edge_appstate::AppState;

use unleash_edge_delta::cache_manager::{DeltaCacheManager, DeltaCacheUpdate};
use unleash_edge_feature_filters::{
    FeatureFilterSet,
    delta_filters::{DeltaFilterSet, combined_filter, filter_delta_events},
    name_prefix_filter, project_filter_from_projects,
};
use unleash_edge_types::{
    EdgeResult, FeatureFilters, TokenCache, errors::EdgeError, tokens::EdgeToken,
};
use unleash_types::client_features::ClientFeaturesDelta;

use crate::get_feature_filter;

struct ClientData {
    token: EdgeToken,
    revision: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct StreamingQuery {
    pub projects: Vec<String>,
    pub name_prefix: Option<String>,
    pub environment: String,
}

impl From<(&unleash_types::client_features::Query, &EdgeToken)> for StreamingQuery {
    fn from((query, token): (&unleash_types::client_features::Query, &EdgeToken)) -> Self {
        Self {
            projects: token.projects.clone(),
            name_prefix: query.name_prefix.clone(),
            environment: match token.environment {
                Some(ref env) => env.clone(),
                None => token.token.clone(),
            },
        }
    }
}

pub fn router() -> Router<AppState> {
    Router::new().route("/streaming", get(stream_deltas))
}

#[axum::debug_handler]
pub async fn stream_deltas(
    State(app_state): State<AppState>,
    edge_token: EdgeToken,
    Query(filter_query): Query<FeatureFilters>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let refresher = app_state.feature_refresher.as_ref().unwrap(); //fix me this won't work
    let token_cache = app_state.token_cache.clone();
    let delta_cache_manager = refresher.delta_cache_manager.clone();

    let (validated_token, _filter_set, query) =
        get_feature_filter(&edge_token, &token_cache, filter_query.clone()).unwrap();

    let rx = delta_cache_manager.subscribe();

    let initial_features = create_event_list(
        &delta_cache_manager,
        0,
        &StreamingQuery::from((&query, &validated_token)),
    )
    .await
    .unwrap();

    let initial_event = Event::default()
        .event("unleash-connected")
        .data(serde_json::to_string(&initial_features).unwrap());

    let intro_stream = once(Ok(initial_event));

    let updates_stream = BroadcastStream::new(rx).filter_map(|broadcast_result| async move {
        let client_data = ClientData {
            token: EdgeToken::default(),
            revision: 0,
        };

        match broadcast_result {
            Ok(DeltaCacheUpdate::Update(env)) => {
                let json = serde_json::to_string(&env).ok()?;
                Some(Ok(Event::default().event("unleash-updated").data(json)))
            }
            Ok(DeltaCacheUpdate::Deletion(env)) => {
                let json = serde_json::to_string(&env).ok()?;
                Some(Ok(Event::default().event("unleash-deleted").data(json)))
            }
            Ok(DeltaCacheUpdate::Full(env)) => {
                let json = serde_json::to_string(&env).ok()?;
                Some(Ok(Event::default()
                    .event("unleash-full-refresh")
                    .data(json)))
            }
            Err(_) => todo!(),
        }
    });

    let full_stream = intro_stream.chain(updates_stream);

    Sse::new(full_stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(30))
            .text("keep-alive"),
    )
}

async fn create_event_list(
    delta_cache_manager: &DeltaCacheManager,
    last_event_id: u32,
    query: &StreamingQuery,
) -> EdgeResult<ClientFeaturesDelta> {
    let filter_set = get_query_filters(&query);
    let delta_filter_set = DeltaFilterSet::default().with_filter(combined_filter(
        last_event_id,
        query.projects.clone(),
        query.name_prefix.clone(),
    ));
    let delta_cache = delta_cache_manager.get(&query.environment);
    match delta_cache {
        Some(delta_cache) => Ok(filter_delta_events(
            &delta_cache,
            &filter_set,
            &delta_filter_set,
            last_event_id,
        )),
        None => {
            // Note: this is a simplification for now, using the following assumptions:
            // 1. We'll only allow streaming in strict mode
            // 2. We'll check whether the token is subsumed *before* trying to add it to the broadcaster
            // If both of these are true, then we should never hit this case (if Thomas's understanding is correct).
            Err(EdgeError::AuthorizationDenied)
        }
    }
}

fn get_query_filters(query: &StreamingQuery) -> FeatureFilterSet {
    if let Some(name_prefix) = &query.name_prefix {
        FeatureFilterSet::from(name_prefix_filter(name_prefix.clone()))
    } else {
        FeatureFilterSet::default()
    }
    .with_filter(project_filter_from_projects(query.projects.clone()))
}
