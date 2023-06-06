use actix_web::{
    post,
    web::{self, Data, Json, ServiceConfig},
};
use chrono::{Duration, Utc};
use dashmap::DashMap;
use rand::distributions::Alphanumeric;
use rand::Rng;
use tracing::{debug, info, instrument};

use crate::http::feature_refresher::FeatureRefresher;
use crate::types::{
    ClientTokenRequest, ClientTokenResponse, EdgeJsonResult, EdgeToken, TokenType,
    TokenValidationStatus,
};

#[post("/api-tokens")]
async fn api_token(
    feature_refresher: Data<FeatureRefresher>,
    token_request: web::Json<ClientTokenRequest>,
) -> EdgeJsonResult<ClientTokenResponse> {
    debug!("Forwarding request to upstream");
    let client_token = feature_refresher
        .forward_request_for_client_token(token_request.into_inner())
        .await?;
    let edge_token = EdgeToken::from(client_token.clone());
    feature_refresher
        .register_token_for_refresh(edge_token, None)
        .await;
    Ok(Json(client_token))
}

#[cfg(not(test))]
pub fn configure_api_token(cfg: &mut ServiceConfig) {
    cfg.service(api_token);
}

#[cfg(test)]
pub fn configure_api_token(cfg: &mut ServiceConfig) {
    info!("Configuring api-token");
    cfg.service(api_token_test);
}

#[cfg(test)]
#[instrument(skip(token_cache, token_req))]
#[post("/api-tokens")]
pub async fn api_token_test(
    token_cache: Data<DashMap<String, EdgeToken>>,
    token_req: Json<ClientTokenRequest>,
) -> EdgeJsonResult<ClientTokenResponse> {
    debug!("Generating client token");
    let req = token_req.into_inner();
    let secret = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(64)
        .map(char::from)
        .collect::<String>();
    let token = format!(
        "{}:{}.{}",
        req.projects.get(0).unwrap(),
        req.environment.clone(),
        secret
    );
    info!("Token generated as {}", token);
    let client_token = EdgeToken {
        token,
        token_type: Some(TokenType::Client),
        environment: Some(req.environment.clone()),
        projects: req.projects.clone(),
        status: TokenValidationStatus::Validated,
    };
    token_cache.insert(client_token.token.clone(), client_token.clone());
    Ok(Json(ClientTokenResponse {
        secret: client_token.token.clone(),
        token_name: "test_generated_token".to_string(),
        token_type: client_token.token_type,
        environment: client_token.environment,
        project: None,
        projects: client_token.projects,
        expires_at: Some(Utc::now() + Duration::weeks(4)),
        created_at: Some(Utc::now()),
        seen_at: None,
        alias: None,
    }))
}
