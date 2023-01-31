use std::{
    future::{ready, Ready},
    str::FromStr,
};

use crate::error::EdgeError;
use actix_web::{
    dev::Payload,
    http::header::{EntityTag, HeaderValue},
    web::{Data, Json},
    FromRequest, HttpRequest,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use shadow_rs::shadow;
use tracing::warn;
use unleash_types::client_features::ClientFeatures;
use unleash_types::client_metrics::{ClientApplication, ClientMetrics};

pub type EdgeJsonResult<T> = Result<Json<T>, EdgeError>;
pub type EdgeResult<T> = Result<T, EdgeError>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TokenType {
    Frontend,
    Client,
    Admin,
}

#[derive(Clone, Debug)]
pub struct ClientFeaturesResponse {
    pub features: Option<ClientFeatures>,
    pub etag: Option<EntityTag>,
}

#[derive(Clone, Debug)]
pub struct ClientFeaturesRequest {
    pub api_key: String,
    pub etag: Option<EntityTag>,
}

pub struct RegisterClientApplicationRequest {
    pub api_key: String,
    pub client_application: ClientApplication,
}

pub struct RegisterClientMetricsRequest {
    pub api_key: String,
    pub client_metrics: ClientMetrics,
}

impl ClientFeaturesRequest {
    pub fn new(api_key: String, etag: Option<String>) -> Self {
        Self {
            api_key,
            etag: etag.map(|tag| EntityTag::new_weak(tag)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EdgeToken {
    pub secret: String,
    #[serde(rename = "type")]
    pub token_type: Option<TokenType>,
    pub environment: Option<String>,
    pub projects: Vec<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub seen_at: Option<DateTime<Utc>>,
    pub alias: Option<String>,
}

impl EdgeToken {
    pub fn no_project_or_environment(s: &str) -> Self {
        EdgeToken {
            secret: s.into(),
            token_type: None,
            environment: None,
            projects: vec![],
            expires_at: None,
            seen_at: Some(Utc::now()),
            alias: None,
        }
    }
}

impl FromRequest for EdgeToken {
    type Error = EdgeError;
    type Future = Ready<EdgeResult<Self>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        if let Some(token_provider) = req.app_data::<Data<dyn EdgeProvider>>() {
            let value = req.headers().get("Authorization");
            let key = match value {
                Some(v) => EdgeToken::try_from(v.clone()),
                None => Err(EdgeError::AuthorizationDenied),
            }
            .and_then(|client_token| {
                if token_provider.secret_is_valid(&client_token.secret)? {
                    Ok(client_token)
                } else {
                    Err(EdgeError::AuthorizationDenied)
                }
            });
            ready(key)
        } else {
            warn!("Could not find a token provider");
            ready(Err(EdgeError::NoTokenProvider))
        }
    }
}

impl TryFrom<HeaderValue> for EdgeToken {
    type Error = EdgeError;

    fn try_from(value: HeaderValue) -> Result<Self, Self::Error> {
        value
            .to_str()
            .map_err(|_| EdgeError::AuthorizationDenied)
            .and_then(EdgeToken::from_str)
    }
}

impl TryFrom<String> for EdgeToken {
    type Error = EdgeError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        EdgeToken::from_str(value.as_str())
    }
}

impl FromStr for EdgeToken {
    type Err = EdgeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.contains(':') && s.contains('.') {
            let token_parts: Vec<String> = s.split(':').take(2).map(|s| s.to_string()).collect();
            let token_projects = if let Some(projects) = token_parts.get(0) {
                if projects == "[]" {
                    vec![]
                } else {
                    vec![projects.clone()]
                }
            } else {
                return Err(EdgeError::TokenParseError);
            };
            if let Some(env_and_key) = token_parts.get(1) {
                let e_a_k: Vec<String> = env_and_key
                    .split('.')
                    .take(2)
                    .map(|s| s.to_string())
                    .collect();
                if e_a_k.len() != 2 {
                    return Err(EdgeError::TokenParseError);
                }
                Ok(EdgeToken {
                    environment: e_a_k.get(0).cloned(),
                    projects: token_projects,
                    token_type: None,
                    secret: s.into(),
                    expires_at: None,
                    seen_at: Some(Utc::now()),
                    alias: None,
                })
            } else {
                Err(EdgeError::TokenParseError)
            }
        } else {
            Ok(EdgeToken::no_project_or_environment(s))
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TokenStrings {
    pub tokens: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ValidatedTokens {
    pub tokens: Vec<EdgeToken>,
}

pub trait FeaturesProvider {
    fn get_client_features(&self, token: EdgeToken) -> EdgeResult<ClientFeatures>;
}

pub trait TokenProvider {
    fn get_known_tokens(&self) -> EdgeResult<Vec<EdgeToken>>;
    fn secret_is_valid(&self, secret: &str) -> EdgeResult<bool>;
    fn token_details(&self, secret: String) -> EdgeResult<Option<EdgeToken>>;
}

pub trait EdgeProvider: FeaturesProvider + TokenProvider + Send + Sync {}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BuildInfo {
    pub package_version: String,
    pub app_name: String,
    pub git_commit_date: DateTime<Utc>,
    pub package_major: String,
    pub package_minor: String,
    pub package_patch: String,
    pub package_version_pre: Option<String>,
    pub branch: String,
    pub tag: String,
    pub rust_version: String,
    pub rust_channel: String,
    pub short_commit_hash: String,
    pub full_commit_hash: String,
    pub build_os: String,
    pub build_target: String,
}

shadow!(build); // Get build information set to build placeholder
impl Default for BuildInfo {
    fn default() -> Self {
        BuildInfo {
            package_version: build::PKG_VERSION.into(),
            app_name: build::PROJECT_NAME.into(),
            package_major: build::PKG_VERSION_MAJOR.into(),
            package_minor: build::PKG_VERSION_MINOR.into(),
            package_patch: build::PKG_VERSION_PATCH.into(),
            package_version_pre: if build::PKG_VERSION_PRE.is_empty() {
                None
            } else {
                Some(build::PKG_VERSION_PRE.into())
            },
            branch: build::BRANCH.into(),
            tag: build::TAG.into(),
            rust_version: build::RUST_VERSION.into(),
            rust_channel: build::RUST_CHANNEL.into(),
            short_commit_hash: build::SHORT_COMMIT.into(),
            full_commit_hash: build::COMMIT_HASH.into(),
            git_commit_date: DateTime::parse_from_rfc3339(build::COMMIT_DATE_3339)
                .expect("shadow-rs did not give proper date")
                .into(),
            build_os: build::BUILD_OS.into(),
            build_target: build::BUILD_TARGET.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use test_case::test_case;
    use tracing::warn;

    use crate::types::EdgeToken;

    #[test_case("943ca9171e2c884c545c5d82417a655fb77cec970cc3b78a8ff87f4406b495d0"; "old java client token")]
    #[test_case("demo-app:production.614a75cf68bef8703aa1bd8304938a81ec871f86ea40c975468eabd6"; "demo token with project and environment")]
    #[test_case("secret-123"; "old example proxy token")]
    #[test_case("*:default.5fa5ac2580c7094abf0d87c68b1eeb54bdc485014aef40f9fcb0673b"; "demo token with access to all projects and default environment")]
    fn edge_token_from_string(token: &str) {
        let parsed_token = EdgeToken::from_str(token);
        match parsed_token {
            Ok(t) => {
                assert_eq!(t.secret, token);
            }
            Err(e) => {
                warn!("{}", e);
                panic!("Could not parse token");
            }
        }
    }
}
