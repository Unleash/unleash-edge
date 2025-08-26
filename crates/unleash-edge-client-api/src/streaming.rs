use std::pin::Pin;
use std::sync::Arc;
use std::{convert::Infallible, time::Duration};

use axum::response::sse::KeepAlive;
use axum::{
    Router,
    extract::{Query, State},
    response::{Sse, sse::Event},
    routing::get,
};
use futures_util::{Stream, StreamExt};
use tokio::sync::RwLock;
use tokio_stream::once;
use tokio_stream::wrappers::BroadcastStream;
use tracing::field::debug;
use tracing::{debug, info};
use unleash_edge_appstate::AppState;

use unleash_edge_delta::cache_manager::{DeltaCacheManager, DeltaCacheUpdate};
use unleash_edge_feature_filters::{
    FeatureFilterSet,
    delta_filters::{DeltaFilterSet, combined_filter, filter_delta_events},
    name_prefix_filter, project_filter_from_projects,
};
use unleash_edge_types::filters::delta;
use unleash_edge_types::{
    EdgeResult, FeatureFilters, TokenCache, errors::EdgeError, tokens::EdgeToken,
};
use unleash_types::client_features::ClientFeaturesDelta;
use unleash_types::client_metrics;

use crate::get_feature_filter;

struct ClientData {
    token: EdgeToken,
    revision: Option<u32>,
    streaming_query: StreamingQuery,
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

fn strip_non_send(
    result: EdgeResult<(
        EdgeToken,
        FeatureFilterSet,
        unleash_types::client_features::Query,
    )>,
) -> EdgeResult<(EdgeToken, unleash_types::client_features::Query)> {
    result.map(|(token, _filter_set, query)| (token, query))
}

pub async fn stream_deltas(
    State(app_state): State<AppState>,
    edge_token: EdgeToken,
    Query(filter_query): Query<FeatureFilters>,
) -> EdgeResult<Sse<impl Stream<Item = Result<Event, axum::Error>>>> {
    let refresher = app_state.feature_refresher.as_ref().unwrap(); //fix me this won't work
    let token_cache = app_state.token_cache.clone();
    let delta_cache_manager = refresher.delta_cache_manager.clone();

    let (validated_token, query) = strip_non_send(get_feature_filter(
        &edge_token,
        &token_cache,
        filter_query.clone(),
    ))?;

    let rx = delta_cache_manager.subscribe();
    let streaming_query = StreamingQuery::from((&query, &validated_token));
    let initial_features =
        create_event_list(delta_cache_manager.clone(), 0, &streaming_query).await?;

    let initial_event = Event::default()
        .event("unleash-connected")
        .data(serde_json::to_string(&initial_features).unwrap());

    let intro_stream = once(Ok(initial_event));
    let client_data = Arc::new(RwLock::new(ClientData {
        token: validated_token.clone(),
        revision: resolve_last_event_id(delta_cache_manager.clone(), &streaming_query),
        streaming_query,
    }));

    let updates_stream = BroadcastStream::new(rx)
        .take_while({
            move |broadcast_result| {
                let should_continue = match &broadcast_result {
                    Ok(DeltaCacheUpdate::Deletion(_)) => false,
                    _ => true,
                };
                Box::pin(async move { should_continue })
                    as Pin<Box<dyn Future<Output = bool> + Send>>
            }
        })
        .filter_map({
            let client_data = client_data.clone();
            let delta_cache_manager = delta_cache_manager.clone();
            move |broadcast_result| {
                let client_data = client_data.clone();
                let delta_cache_manager = delta_cache_manager.clone();
                Box::pin(async move {
                    match broadcast_result {
                        Ok(DeltaCacheUpdate::Update(_)) => {
                            let mut client_data = client_data.write().await;
                            let streaming_query = &client_data.streaming_query;

                            let event_list = create_event_list(
                                delta_cache_manager.clone(),
                                client_data.revision.unwrap_or_default(),
                                streaming_query,
                            )
                            .await
                            .unwrap();

                            let last_event_id =
                                resolve_last_event_id(delta_cache_manager, streaming_query);
                            client_data.revision = last_event_id;

                            Some(
                                Event::default()
                                    .event("unleash-updated")
                                    .json_data(&event_list),
                            )
                        }
                        _ => None,
                    }
                }) as Pin<Box<dyn Future<Output = _> + Send>>
            }
        });

    let full_stream = intro_stream.chain(updates_stream);

    Ok(Sse::new(full_stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(30))
            .text("keep-alive"),
    ))
}

async fn create_event_list(
    delta_cache_manager: Arc<DeltaCacheManager>,
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

fn resolve_last_event_id(
    delta_cache_manager: Arc<DeltaCacheManager>,
    query: &StreamingQuery,
) -> Option<u32> {
    let delta_cache = delta_cache_manager.get(&query.environment);
    match delta_cache {
        Some(delta_cache) => delta_cache
            .get_events()
            .last()
            .map(|event| event.get_event_id()),
        None => None,
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
