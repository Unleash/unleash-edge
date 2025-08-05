use std::sync::Arc;
use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use tracing::instrument;
use unleash_edge_appstate::AppState;
use unleash_edge_types::tokens::{EdgeToken, TokenStrings, ValidatedTokens};
use unleash_edge_types::{EdgeJsonResult, TokenValidationStatus};

#[utoipa::path(
    post,
    path = "/edge/validate",
    responses(
        (status = 200, description = "Return valid tokens from list of tokens passed in to validate", body = ValidatedTokens)
    ),
    request_body = TokenStrings
)]
#[instrument(skip(app_state, tokens))]
pub async fn validate(
    app_state: State<AppState>,
    tokens: Json<TokenStrings>,
) -> EdgeJsonResult<ValidatedTokens> {
    match *app_state.token_validator {
        Some(ref validator) => {
            let known_tokens = validator.register_tokens(tokens.tokens.clone()).await?;
            Ok(Json(ValidatedTokens {
                tokens: known_tokens
                    .into_iter()
                    .filter(|t| t.status == TokenValidationStatus::Validated)
                    .collect(),
            }))
        }
        None => {
            let tokens_to_check = tokens.tokens.clone();
            let valid_tokens: Vec<EdgeToken> = tokens_to_check
                .iter()
                .filter_map(|t| app_state.token_cache.get(t).map(|e| e.value().clone()))
                .collect();
            Ok(Json(ValidatedTokens {
                tokens: valid_tokens,
            }))
        }
    }
}

pub fn router() -> Router<AppState> {
    Router::new().route("/validate", post(validate))
}
