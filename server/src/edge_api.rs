use actix_web::{
    get,
    web::{self, Json},
};

use crate::types::{EdgeJsonResult, EdgeToken, TokenProvider, TokenStrings, ValidatedTokens};

#[get("/validate")]
async fn validate(
    _client_token: EdgeToken,
    token_provider: web::Data<dyn TokenProvider>,
    tokens: Json<TokenStrings>,
) -> EdgeJsonResult<ValidatedTokens> {
    let valid_tokens: Vec<EdgeToken> = tokens
        .into_inner()
        .tokens
        .into_iter()
        .filter(|t| token_provider.secret_is_valid(t))
        .map(|t| token_provider.token_details(t).unwrap()) // Guaranteed because we just checked that the secret exists
        .collect();
    Ok(Json(ValidatedTokens {
        tokens: valid_tokens,
    }))
}

pub fn configure_edge_api(cfg: &mut web::ServiceConfig) {
    cfg.service(validate);
}
