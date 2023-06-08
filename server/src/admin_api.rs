use actix_web::{
    post,
    web::{self, Data, Json, ServiceConfig},
};
use tracing::{debug, instrument};

use crate::http::feature_refresher::FeatureRefresher;
use crate::types::{ClientTokenRequest, ClientTokenResponse, EdgeJsonResult, EdgeToken};

#[post("/api-tokens")]
#[instrument(skip(feature_refresher, token_request))]
async fn api_token(
    feature_refresher: Data<FeatureRefresher>,
    token_request: Json<ClientTokenRequest>,
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
pub fn configure_admin_api(cfg: &mut ServiceConfig) {
    cfg.service(web::scope("/admin").service(api_token));
}

#[cfg(test)]
pub fn configure_admin_api(cfg: &mut ServiceConfig) {
    cfg.service(web::scope("/admin").service(tests::api_token_test));
}

#[cfg(test)]
mod tests {
    use crate::tests::features_from_disk;
    use crate::tokens::cache_key;
    use crate::types::{
        ClientTokenRequest, ClientTokenResponse, EdgeJsonResult, EdgeToken, TokenType,
        TokenValidationStatus,
    };
    use actix_web::post;
    use actix_web::web::{Data, Json};
    use chrono::{Duration, Utc};
    use dashmap::DashMap;
    use rand::distributions::Alphanumeric;
    use rand::Rng;
    use tracing::{debug, instrument};
    use unleash_types::client_features::ClientFeatures;

    pub fn client_token(environment: String, projects: Vec<String>) -> EdgeToken {
        let secret = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(64)
            .map(char::from)
            .collect::<String>();
        let project_token = match projects.len() {
            0 => "*",
            1 => projects.get(0).unwrap(),
            _ => {
                if projects.contains(&"*".to_string()) {
                    "*"
                } else {
                    "[]"
                }
            }
        };
        let token = format!("{}:{}.{}", project_token, environment, secret);
        EdgeToken {
            token,
            token_type: Some(TokenType::Client),
            environment: Some(environment),
            projects,
            status: TokenValidationStatus::Validated,
        }
    }

    #[instrument(skip(token_cache, token_req, features_cache))]
    #[post("/api-tokens")]
    pub async fn api_token_test(
        token_cache: Data<DashMap<String, EdgeToken>>,
        features_cache: Data<DashMap<String, ClientFeatures>>,
        token_req: Json<ClientTokenRequest>,
    ) -> EdgeJsonResult<ClientTokenResponse> {
        debug!("Generating client token");
        let req = token_req.into_inner();
        let features = features_from_disk("../examples/features.json");
        let client_token = client_token(req.environment.clone(), req.projects);
        debug!("Token generated as {:?}", client_token);
        features_cache.insert(cache_key(&client_token), features);
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
}
