use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use axum::response::{Sse, sse::Event};
use futures_util::{Stream, StreamExt};
use tokio::sync::RwLock;
use tokio_stream::once;
use tokio_stream::wrappers::BroadcastStream;

use unleash_edge_delta::cache_manager::{DeltaCacheManager, DeltaCacheUpdate};
use unleash_edge_feature_filters::{
    FeatureFilterSet,
    delta_filters::{DeltaFilterSet, combined_filter, filter_delta_events},
    get_feature_filter, name_prefix_filter, project_filter_from_projects,
};
use unleash_edge_types::{
    EdgeResult, FeatureFilters, TokenCache, errors::EdgeError, tokens::EdgeToken,
};
use unleash_types::client_features::ClientFeaturesDelta;

pub async fn stream_deltas(
    delta_cache_manager: Arc<DeltaCacheManager>,
    token_cache: Arc<TokenCache>,
    edge_token: EdgeToken,
    filter_query: FeatureFilters,
) -> EdgeResult<Sse<impl Stream<Item = Result<Event, axum::Error>>>> {
    let token_cache = token_cache.clone();

    let (validated_token, query) =
        get_feature_filter(&edge_token, &token_cache, filter_query.clone())
            .map(|(token, _filter_set, query)| (token, query))?;

    let rx = delta_cache_manager.subscribe();
    let streaming_query = StreamingQuery::from((&query, &validated_token));

    let initial_features =
        create_event_list(delta_cache_manager.clone(), 0, &streaming_query).await?;

    let mut initial_event = Event::default().event("unleash-connected");
    if let Some(event_id) = event_list_last_id(&initial_features) {
        initial_event = initial_event.id(event_id.to_string());
    }
    let initial_event = initial_event.json_data(&initial_features);

    let intro_stream = once(initial_event);
    let client_data = Arc::new(RwLock::new(ClientData {
        revision: resolve_last_event_id(delta_cache_manager.clone(), &streaming_query),
        streaming_query,
    }));

    let updates_stream = BroadcastStream::new(rx)
        .take_while({
            move |broadcast_result| {
                let environment_not_deleted =
                    !matches!(broadcast_result, Ok(DeltaCacheUpdate::Deletion(_)));
                let token_valid = if let Some(known_token) = token_cache.get(&edge_token.token) {
                    known_token.value().status.is_valid()
                } else {
                    false
                };

                let should_continue = environment_not_deleted && token_valid;

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
                        Ok(DeltaCacheUpdate::Update(environment)) => {
                            if environment != client_data.read().await.streaming_query.environment {
                                return None;
                            }

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

                            let mut event = Event::default().event("unleash-updated");
                            if let Some(event_id) = event_list_last_id(&event_list) {
                                event = event.id(event_id.to_string());
                            }

                            Some(event.json_data(&event_list))
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
    let filter_set = get_query_filters(query);
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
        None => Err(EdgeError::AuthorizationDenied),
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

fn event_list_last_id(event_list: &ClientFeaturesDelta) -> Option<u32> {
    event_list.events.last().map(|event| event.get_event_id())
}

fn get_query_filters(query: &StreamingQuery) -> FeatureFilterSet {
    if let Some(name_prefix) = &query.name_prefix {
        FeatureFilterSet::from(name_prefix_filter(name_prefix.clone()))
    } else {
        FeatureFilterSet::default()
    }
    .with_filter(project_filter_from_projects(query.projects.clone()))
}

struct ClientData {
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
