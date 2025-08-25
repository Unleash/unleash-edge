use std::convert::Infallible;

use axum::{
    Router,
    extract::{Query, State},
    response::{Sse, sse::Event},
    routing::get,
};
use dashmap::DashMap;
use futures_util::Stream;
use tokio::{
    stream,
    sync::{broadcast, mpsc::Receiver},
};
use unleash_edge_appstate::AppState;

use unleash_edge_streaming::stream_broadcast::Broadcaster;
use unleash_edge_types::{
    EdgeResult, FeatureFilters, TokenCache, errors::EdgeError, tokens::EdgeToken,
};

use crate::get_feature_filter;

pub async fn stream_features(
    app_state: State<AppState>,
    edge_token: EdgeToken,
    filter_query: Query<FeatureFilters>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, EdgeError> {
    let broadcaster = app_state.streaming_broadcaster.clone();

    match broadcaster {
        Some(broadcaster) => {
            let (validated_token, _filter_set, query) =
                get_feature_filter(&edge_token, &app_state.token_cache, filter_query.clone())?;

            broadcaster.connect(validated_token, query).await
        }
        None => Err(EdgeError::Forbidden(
            "This endpoint is only enabled in streaming mode".into(),
        )),
    }
}

pub fn router() -> Router<AppState> {
    Router::new().route("/streaming", get(stream_features))
}
