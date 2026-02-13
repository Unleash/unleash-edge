use std::sync::Arc;

use axum::{
    Router,
    extract::{FromRef, Query, State},
    http::HeaderMap,
    response::{Sse, sse::Event},
    routing::get,
};
use futures_util::Stream;

use unleash_edge_appstate::{
    AppState,
    edge_token_extractor::{AuthState, AuthToken},
};
use unleash_edge_delta::cache_manager::DeltaCacheManager;
use unleash_edge_streaming::stream_broadcast::stream_deltas;
use unleash_edge_types::{EdgeResult, FeatureFilters, TokenCache, errors::EdgeError};

pub fn streaming_router_for<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
    StreamingState: FromRef<S>,
    AuthState: FromRef<S>,
{
    Router::new().route("/streaming", get(setup_streaming))
}

pub fn router() -> Router<AppState> {
    streaming_router_for::<AppState>()
}

#[derive(Clone)]
pub struct StreamingState {
    pub delta_cache_manager: Option<Arc<DeltaCacheManager>>,
    pub token_cache: Arc<TokenCache>,
}

impl FromRef<AppState> for StreamingState {
    fn from_ref(app: &AppState) -> Self {
        Self {
            delta_cache_manager: app.delta_cache_manager.clone(),
            token_cache: app.token_cache.clone(),
        }
    }
}

async fn setup_streaming(
    State(app_state): State<StreamingState>,
    AuthToken(edge_token): AuthToken,
    Query(filter_query): Query<FeatureFilters>,
    headers: HeaderMap,
) -> EdgeResult<Sse<impl Stream<Item = Result<Event, axum::Error>>>> {
    let Some(delta_cache_manager) = app_state.delta_cache_manager.as_ref() else {
        return Err(EdgeError::SseError(
            "No delta cache manager found, streaming will not work. This is likely because Edge was not started in streaming mode.".into(),
        ));
    };

    let last_event_id_header = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok());
    let last_event_id = last_event_id_header.and_then(|value| value.parse::<u32>().ok());
    if last_event_id_header.is_some() && last_event_id.is_none() {
        tracing::debug!("Invalid last-event-id header value");
    } else if let Some(id) = last_event_id {
        tracing::debug!("Client provided last-event-id={id}");
    }

    stream_deltas(
        delta_cache_manager.clone(),
        app_state.token_cache.clone(),
        edge_token,
        filter_query,
        last_event_id,
    )
    .await
}
