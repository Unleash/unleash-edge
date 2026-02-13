use base64::Engine as _;
use base64::prelude::BASE64_URL_SAFE_NO_PAD;
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use http::StatusCode;
use rand::{RngExt, rng};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::warn;
use unleash_edge_types::errors::EdgeError;
use unleash_edge_types::tokens::{EdgeToken, RequestTokensArg};
use unleash_edge_types::{EdgeResult, EdgeTokens};

type HmacSha256 = Hmac<Sha256>;

const CLIENT_ID: &str = "enterprise-edge";

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
        client,
        environments,
        client_secret,
        issue_token_url,
    }: RequestTokensArg,
) -> EdgeResult<Vec<EdgeToken>> {
    let desired_tokens = environments
        .iter()
        .map(|environment| TokenRequest {
            environment: environment.clone(),
            projects: vec!["*".to_string()],
        })
        .collect();
    let request_body = HmacTokenRequest {
        tokens: desired_tokens,
    };

    let timestamp = Utc::now();
    let mut nonce = [0u8; 16];
    rng().fill(&mut nonce);
    let nonce_as_hex = hash_to_string(nonce.iter());
    let content_string = serde_json::to_string(&request_body)
        .map_err(|e| EdgeError::JsonParseError(e.to_string()))?;
    let mut content_hasher = Sha256::new();
    content_hasher.update(&content_string);
    let finalized_hash = content_hasher.finalize();
    let hash_as_hex = hash_to_string(finalized_hash.iter());
    let signature = create_canonical_signature(
        &client_secret,
        &timestamp,
        &nonce_as_hex,
        issue_token_url.path(),
        &hash_as_hex,
    )?;

    let response = client
        .post(issue_token_url)
        .header(
            reqwest::header::AUTHORIZATION,
            format!("HMAC {}:{}", CLIENT_ID, signature),
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
            Ok(token_response.tokens)
        }
        s => {
            warn!("Failed to validate HMAC request.");
            Err(EdgeError::HmacTokenResponseError(
                response
                    .text()
                    .await
                    .unwrap_or(format!("Failed to validate HMAC request, status code {s}"))
                    .to_string(),
            ))
        }
    }
}

fn create_canonical_signature(
    client_secret: &str,
    timestamp: &DateTime<Utc>,
    nonce: &str,
    path: &str,
    content_hash: &str,
) -> EdgeResult<String> {
    let canonical_request = format!(
        "{}\n{}\n{}\n{}\n{}",
        "POST",
        path,
        timestamp.to_rfc3339(),
        nonce,
        content_hash
    );
    let key = BASE64_URL_SAFE_NO_PAD
        .decode(client_secret.as_bytes())
        .map_err(|_e| EdgeError::HmacSignatureError)?;
    let mut signature =
        HmacSha256::new_from_slice(&key).map_err(|_e| EdgeError::HmacSignatureError)?;
    signature.update(canonical_request.as_bytes());
    Ok(BASE64_URL_SAFE_NO_PAD.encode(signature.finalize().into_bytes()))
}

fn hash_to_string<'a, I>(vals: I) -> String
where
    I: Iterator<Item = &'a u8>,
{
    vals.map(|v| format!("{v:02x}")).collect::<String>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::post;
    use axum::{Json, Router};
    use chrono::DateTime;
    use http::HeaderMap;
    use rand::{RngExt, rng};
    use unleash_edge_types::tokens::EdgeToken;
    use unleash_edge_types::{TokenType, TokenValidationStatus};

    const CLIENT_ID: &str = "enterprise-edge";
    const CLIENT_SECRET: &str = "koom8ceiGaeBee9Eivahweideimak4aV";

    fn content_hash(content: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        hash_to_string(hasher.finalize().iter())
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
        rng().fill(&mut bytes);
        hash_to_string(bytes.iter())
    }
    async fn validate_token_request(
        headers: HeaderMap,
        Json(body): Json<HmacTokenRequest>,
    ) -> Json<EdgeTokens> {
        assert!(headers.contains_key("x-timestamp"));
        assert!(headers.contains_key("x-nonce"));
        assert_eq!(headers.get("content-type").unwrap(), "application/json");
        let expected_content_hash = content_hash(&serde_json::to_string(&body).unwrap());
        assert_eq!(
            headers.get("content-sha256").unwrap().to_str().unwrap(),
            expected_content_hash.as_str()
        );
        let timestamp_as_str = headers
            .get("x-timestamp")
            .and_then(|t| t.to_str().ok())
            .expect("Failed to extract timestamp header");
        let timestamp = DateTime::parse_from_rfc3339(timestamp_as_str)
            .expect("Failed to convert timestamp")
            .to_utc();
        let nonce_as_str = headers
            .get("x-nonce")
            .and_then(|t| t.to_str().ok())
            .expect("Failed to extract nonce");
        let signature = create_canonical_signature(
            CLIENT_SECRET,
            &timestamp,
            nonce_as_str,
            "/edge/issue-token",
            &expected_content_hash,
        )
        .expect("Could not create signature");
        assert_eq!(
            headers.get("authorization").unwrap().to_str().unwrap(),
            format!("HMAC {}:{}", CLIENT_ID, signature)
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
        let client = reqwest::Client::new();
        let tokens = request_tokens(RequestTokensArg {
            client,
            environments: vec!["development".into(), "production".into()],
            issue_token_url: url,
            client_secret: CLIENT_SECRET.into(),
        })
        .await
        .unwrap();
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].environment, Some("development".into()));
        assert_eq!(tokens[1].environment, Some("production".into()));
    }
}
