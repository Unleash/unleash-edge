use tracing::instrument;
use unleash_types::client_features::ClientFeatures;

// #[utoipa::path(
//     get,
//     path = "/features",
//     context_path = "/api/client",
//     params(FeatureFilters),
//     responses(
//         (status = 200, description = "Return feature toggles for this token", body = ClientFeatures),
//         (status = 403, description = "Was not allowed to access features"),
//         (status = 400, description = "Invalid parameters used")
//     ),
//     security(
//         ("Authorization" = [])
//     )
// )]
// #[instrument(skip(app_state, edge_token, filter_query))]
// pub async fn stream_features(
//     edge_token: EdgeToken,
//     token_cache: Data<DashMap<String, EdgeToken>>,
//     edge_mode: Data<EdgeMode>,
//     filter_query: Query<FeatureFilters>,
// ) -> EdgeJsonResult<ClientFeatures> {
//     resolve_features(&app_state, edge_token.clone(), filter_query.0.clone()).await
// }
