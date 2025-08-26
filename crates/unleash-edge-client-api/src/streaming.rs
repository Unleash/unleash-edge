use axum::{
    Router,
    extract::{Query, State},
    response::{Sse, sse::Event},
    routing::get,
};
use futures_util::Stream;

use unleash_edge_appstate::AppState;
use unleash_edge_streaming::stream_broadcast::stream_deltas;
use unleash_edge_types::{EdgeResult, FeatureFilters, errors::EdgeError, tokens::EdgeToken};

pub fn router() -> Router<AppState> {
    Router::new().route("/streaming", get(setup_streaming))
}

async fn setup_streaming(
    State(app_state): State<AppState>,
    edge_token: EdgeToken,
    Query(filter_query): Query<FeatureFilters>,
) -> EdgeResult<Sse<impl Stream<Item = Result<Event, axum::Error>>>> {
    let Some(refresher) = app_state.feature_refresher.as_ref() else {
        return Err(EdgeError::SseError(
            "No feature refresher found, streaming will not work, this is an application error, please report it.".into(),
        ));
    };

    stream_deltas(
        refresher.clone(),
        app_state.token_cache.clone(),
        edge_token,
        filter_query,
    )
    .await
}
