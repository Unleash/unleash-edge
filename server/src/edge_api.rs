use actix_web::{
    get,
    web::{self, Json},
};

use crate::types::{EdgeJsonResult, EdgeSource, EdgeToken, TokenStrings, ValidatedTokens};

#[get("/validate")]
async fn validate(
    _client_token: EdgeToken,
    token_provider: web::Data<dyn EdgeSource>,
    tokens: Json<TokenStrings>,
) -> EdgeJsonResult<ValidatedTokens> {
    let valid_tokens = token_provider
        .get_valid_tokens(tokens.into_inner().tokens)
        .await?;
    Ok(Json(ValidatedTokens {
        tokens: valid_tokens,
    }))
}

pub fn configure_edge_api(cfg: &mut web::ServiceConfig) {
    cfg.service(validate);
}
