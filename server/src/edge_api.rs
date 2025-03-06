use actix_web::{
    HttpRequest, post,
    web::{self, Data, Json},
};
use dashmap::DashMap;
use utoipa;

use crate::auth::token_validator::TokenValidator;
use crate::types::{
    EdgeJsonResult, EdgeToken, TokenStrings, TokenValidationStatus, ValidatedTokens,
};

#[utoipa::path(
    path = "/edge/validate",
    responses(
        (status = 200, description = "Return valid tokens from list of tokens passed in to validate", body = ValidatedTokens)
    ),
    request_body = TokenStrings
)]
#[post("/validate")]
pub async fn validate(
    token_cache: web::Data<DashMap<String, EdgeToken>>,
    req: HttpRequest,
    tokens: Json<TokenStrings>,
) -> EdgeJsonResult<ValidatedTokens> {
    let maybe_validator = req.app_data::<Data<TokenValidator>>();
    match maybe_validator {
        Some(validator) => {
            let known_tokens = validator
                .register_tokens(tokens.into_inner().tokens)
                .await?;
            Ok(Json(ValidatedTokens {
                tokens: known_tokens
                    .into_iter()
                    .filter(|t| t.status == TokenValidationStatus::Validated)
                    .collect(),
            }))
        }
        None => {
            let tokens_to_check = tokens.into_inner().tokens;
            let valid_tokens: Vec<EdgeToken> = tokens_to_check
                .iter()
                .filter_map(|t| token_cache.get(t).map(|e| e.value().clone()))
                .collect();
            Ok(Json(ValidatedTokens {
                tokens: valid_tokens,
            }))
        }
    }
}

pub fn configure_edge_api(cfg: &mut web::ServiceConfig) {
    cfg.service(validate);
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use actix_web::http::header::ContentType;
    use actix_web::web::Json;
    use actix_web::{App, test, web};
    use dashmap::DashMap;

    use crate::auth::token_validator::TokenValidator;
    use crate::edge_api::validate;
    use crate::types::{
        EdgeToken, TokenStrings, TokenType, TokenValidationStatus, ValidatedTokens,
    };

    #[tokio::test]
    pub async fn validating_incorrect_tokens_returns_empty_list() {
        let token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());
        let app = test::init_service(
            App::new()
                .app_data(web::Data::from(token_cache.clone()))
                .service(web::scope("/edge").service(validate)),
        )
        .await;
        let mut valid_token =
            EdgeToken::try_from("test-app:development.abcdefghijklmnopqrstu".to_string()).unwrap();
        valid_token.token_type = Some(TokenType::Client);
        valid_token.status = TokenValidationStatus::Validated;
        token_cache.insert(valid_token.token.clone(), valid_token.clone());
        let token_strings = TokenStrings {
            tokens: vec!["random_token:rqweqwew.qweqwjeqwkejlqwe".into()],
        };
        let req = test::TestRequest::post()
            .uri("/edge/validate")
            .insert_header(ContentType::json())
            .set_json(Json(token_strings))
            .to_request();
        let res: ValidatedTokens = test::call_and_read_body_json(&app, req).await;
        assert_eq!(res.tokens.len(), 0);
    }

    #[tokio::test]
    pub async fn validating_a_mix_of_tokens_only_returns_valid_tokens() {
        let token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());
        let app = test::init_service(
            App::new()
                .app_data(web::Data::from(token_cache.clone()))
                .service(web::scope("/edge").service(validate)),
        )
        .await;
        let mut valid_token =
            EdgeToken::try_from("test-app:development.abcdefghijklmnopqrstu".to_string()).unwrap();
        valid_token.token_type = Some(TokenType::Client);
        valid_token.status = TokenValidationStatus::Validated;
        token_cache.insert(valid_token.token.clone(), valid_token.clone());

        let token_strings = TokenStrings {
            tokens: vec![
                "test-app:development.abcdefghijklmnopqrstu".into(),
                "probablyaninvalidproject:development.some_crazy_secret".into(),
            ],
        };
        let req = test::TestRequest::post()
            .uri("/edge/validate")
            .insert_header(ContentType::json())
            .set_json(Json(token_strings))
            .to_request();
        let res: ValidatedTokens = test::call_and_read_body_json(&app, req).await;
        assert_eq!(res.tokens.len(), 1);
        assert!(res.tokens.iter().any(|t| t.token == valid_token.token));
    }

    #[tokio::test]
    pub async fn adding_a_token_validator_filters_so_only_validated_tokens_are_returned() {
        let token_cache: Arc<DashMap<String, EdgeToken>> = Arc::new(DashMap::default());
        let token_validator = TokenValidator {
            unleash_client: Arc::new(Default::default()),
            token_cache: token_cache.clone(),
            persistence: None,
        };
        let app = test::init_service(
            App::new()
                .app_data(web::Data::from(token_cache.clone()))
                .app_data(web::Data::new(token_validator))
                .service(web::scope("/edge").service(validate)),
        )
        .await;
        let mut valid_token =
            EdgeToken::try_from("test-app:development.abcdefghijklmnopqrstu".to_string()).unwrap();
        valid_token.token_type = Some(TokenType::Client);
        valid_token.status = TokenValidationStatus::Validated;
        let mut invalid_token = EdgeToken::try_from(
            "probablyaninvalidproject:development.some_crazy_secret".to_string(),
        )
        .unwrap();
        invalid_token.status = TokenValidationStatus::Invalid;
        invalid_token.token_type = Some(TokenType::Admin);
        token_cache.insert(valid_token.token.clone(), valid_token.clone());
        token_cache.insert(invalid_token.token.clone(), invalid_token.clone());
        let token_strings = TokenStrings {
            tokens: vec![
                "test-app:development.abcdefghijklmnopqrstu".into(),
                "probablyaninvalidproject:development.some_crazy_secret".into(),
            ],
        };
        let req = test::TestRequest::post()
            .uri("/edge/validate")
            .insert_header(ContentType::json())
            .set_json(Json(token_strings))
            .to_request();
        let res: ValidatedTokens = test::call_and_read_body_json(&app, req).await;
        assert_eq!(res.tokens.len(), 1);
        assert!(res.tokens.iter().any(|t| t.token == valid_token.token));
    }
}
