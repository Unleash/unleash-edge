use std::sync::Arc;

use axum::extract::{FromRef, State};
use axum::routing::post;
use axum::{Json, Router};
use tracing::instrument;
use unleash_edge_appstate::AppState;
use unleash_edge_auth::token_validator::TokenValidator;
use unleash_edge_types::tokens::{EdgeToken, TokenStrings, ValidatedTokens};
use unleash_edge_types::{EdgeJsonResult, TokenCache, TokenValidationStatus};

#[derive(Clone)]
pub struct EdgeApiState {
    pub token_cache: Arc<TokenCache>,
    pub token_validator: Arc<Option<TokenValidator>>,
}

impl FromRef<AppState> for EdgeApiState {
    fn from_ref(app_state: &AppState) -> Self {
        EdgeApiState {
            token_cache: app_state.token_cache.clone(),
            token_validator: app_state.token_validator.clone(),
        }
    }
}

#[utoipa::path(
    post,
    path = "/edge/validate",
    responses(
        (status = 200, description = "Return valid tokens from list of tokens passed in to validate", body = ValidatedTokens)
    ),
    request_body = TokenStrings
)]
#[instrument(skip(app_state, tokens))]
async fn validate(
    app_state: State<EdgeApiState>,
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
                .map(|mut token| {
                    token.status = TokenValidationStatus::Validated;
                    token
                })
                .collect();
            Ok(Json(ValidatedTokens {
                tokens: valid_tokens,
            }))
        }
    }
}

pub fn edge_api_router_for<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
    EdgeApiState: FromRef<S>,
{
    Router::new().route("/validate", post(validate))
}

pub fn router() -> Router<AppState> {
    edge_api_router_for::<AppState>()
}
