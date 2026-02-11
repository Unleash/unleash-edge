use base64::Engine as _;
use base64::prelude::BASE64_URL_SAFE_NO_PAD;
use chrono::Utc;
use hmac::{Hmac, Mac};
use http::StatusCode;
use prometheus::{IntGaugeVec, Opts, register_int_gauge_vec};
use rand::{RngCore, rng};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::LazyLock;
use tracing::warn;
use unleash_edge_types::errors::EdgeError;
use unleash_edge_types::tokens::{EdgeToken, RequestTokensArg};
use unleash_edge_types::{EdgeResult, EdgeTokens, TokenValidationStatus};

type HmacSha256 = Hmac<Sha256>;

static HMAC_TOKEN_REQUEST_FAILURES: LazyLock<IntGaugeVec> = LazyLock::new(|| {
    register_int_gauge_vec!(
        Opts::new(
            "hmac_token_request_failures",
            "why we failed to validate hmac"
        ),
        &["status_code"]
    )
    .unwrap()
});

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TokenRequest {
    pub environment: String,
    pub projects: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HmacTokenRequest {
    tokens: Vec<TokenRequest>,
}

pub async fn request_tokens(
    RequestTokensArg {
        environments,
        projects,
        client_id,
        client_secret,
        issue_token_url,
    }: RequestTokensArg,
) -> EdgeResult<Vec<EdgeToken>> {
    let desired_tokens = environments
        .iter()
        .map(|environment| TokenRequest {
            environment: environment.clone(),
            projects: projects.clone(),
        })
        .collect();
    let request_body = HmacTokenRequest {
        tokens: desired_tokens,
    };

    let timestamp = Utc::now();
    let mut nonce = [0u8; 16];
    rng().fill_bytes(&mut nonce);
    let nonce_as_hex = hex::encode(nonce);
    let content_string = serde_json::to_string(&request_body)
        .map_err(|e| EdgeError::JsonParseError(e.to_string()))?;
    let mut content_hasher = Sha256::new();
    content_hasher.update(&content_string);
    let finalized_hash = content_hasher.finalize();
    let hash_as_hex = hex::encode(finalized_hash);
    let canonical_request = format!(
        "POST\n{}\n{}\n{}\n{}",
        "/edge/issue-token",
        timestamp.to_rfc3339(),
        nonce_as_hex,
        hash_as_hex
    );
    let key = BASE64_URL_SAFE_NO_PAD
        .decode(client_secret.as_bytes())
        .map_err(|_e| EdgeError::HmacSignatureError)?;
    let mut signature =
        HmacSha256::new_from_slice(&key).map_err(|_e| EdgeError::HmacSignatureError)?;
    signature.update(canonical_request.as_bytes());
    let url_safe_signature_base64 =
        BASE64_URL_SAFE_NO_PAD.encode(signature.finalize().into_bytes());
    let client = Client::new();
    let response = client
        .post(issue_token_url)
        .header(
            reqwest::header::AUTHORIZATION,
            format!("HMAC {}:{}", client_id, url_safe_signature_base64),
        )
        .header("x-timestamp", timestamp.to_rfc3339())
        .header("x-nonce", nonce_as_hex)
        .header("content-sha256", hash_as_hex)
        .header("Content-Type", "application/json")
        .body(content_string)
        .send()
        .await
        .map_err(|e| EdgeError::HmacTokenRequestError(e.to_string()))?;
    match response.status() {
        StatusCode::OK => {
            let token_response = response
                .json::<EdgeTokens>()
                .await
                .map_err(|e| EdgeError::HmacTokenResponseError(e.to_string()))?;
            Ok(token_response
                .tokens
                .into_iter()
                .map(|t| {
                    let remaining_info =
                        EdgeToken::try_from(t.token.clone()).unwrap_or_else(|_| t.clone());
                    EdgeToken {
                        token: t.token.clone(),
                        token_type: t.token_type,
                        environment: t.environment.or(remaining_info.environment),
                        projects: t.projects,
                        status: TokenValidationStatus::Validated,
                    }
                })
                .collect())
        }
        s => {
            HMAC_TOKEN_REQUEST_FAILURES
                .with_label_values(&[s.as_str()])
                .inc();
            warn!("Failed to validate HMAC request.");
            Err(EdgeError::HmacTokenResponseError(s.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::post;
    use axum::{Json, Router};
    use http::HeaderMap;
    use unleash_edge_types::TokenType;
    use unleash_edge_types::tokens::EdgeToken;

    fn content_hash(content: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        hex::encode(hasher.finalize())
    }

    impl TokenRequest {
        pub fn generated_token(&self) -> String {
            format!(
                "{}:{}.{}",
                if self.projects.len() > 1 {
                    "[]"
                } else {
                    self.projects[0].as_str()
                },
                self.environment.as_str(),
                generate_random(26)
            )
        }
    }
    fn generate_random(length: usize) -> String {
        let mut bytes = Vec::with_capacity(length);
        rng().fill_bytes(&mut bytes);
        hex::encode(bytes)
    }
    async fn validate_token_request(
        headers: HeaderMap,
        Json(body): Json<HmacTokenRequest>,
    ) -> Json<EdgeTokens> {
        assert!(headers.contains_key("x-timestamp"));
        assert!(headers.contains_key("x-nonce"));
        assert_eq!(headers.get("content-type").unwrap(), "application/json");
        assert!(
            headers
                .get("authorization")
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("HMAC client_id:")
        );
        let expected_content_hash = content_hash(&serde_json::to_string(&body).unwrap());
        assert_eq!(
            headers.get("content-sha256").unwrap().to_str().unwrap(),
            expected_content_hash.as_str()
        );
        let tokens: Vec<EdgeToken> = body
            .tokens
            .iter()
            .map(|requested| EdgeToken {
                token: requested.generated_token(),
                token_type: Some(TokenType::Backend),
                environment: Some(requested.environment.clone()),
                projects: requested.projects.clone(),
                status: TokenValidationStatus::Validated,
            })
            .collect();
        let edge_tokens = EdgeTokens { tokens };
        Json(edge_tokens)
    }

    #[tokio::test]
    pub async fn makes_hmac_request_based_on_props() {
        let router = Router::new().route("/edge/issue-token", post(validate_token_request));

        let ts = axum_test::TestServer::builder()
            .http_transport()
            .build(router)
            .expect("Failed to build test server");
        let url = ts.server_url("/edge/issue-token").unwrap();
        let tokens = request_tokens(RequestTokensArg {
            environments: vec!["development".into(), "production".into()],
            projects: vec!["*".into()],
            issue_token_url: url,
            client_id: "client_id".into(),
            client_secret: "koom8ceiGaeBee9Eivahweideimak4aV".into(),
        })
        .await
        .unwrap();
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].environment, Some("development".into()));
        assert_eq!(tokens[1].environment, Some("production".into()));
    }
}
