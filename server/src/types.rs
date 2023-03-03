use std::fmt;
use std::sync::Arc;
use std::{
    future::{ready, Ready},
    hash::{Hash, Hasher},
    str::FromStr,
};

use crate::cli::EdgeMode;
use crate::error::EdgeError;
use actix_web::web::Data;
use actix_web::{
    dev::Payload,
    http::header::{EntityTag, HeaderValue},
    web::Json,
    FromRequest, HttpRequest,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use shadow_rs::shadow;
use unleash_types::client_features::ClientFeatures;
use unleash_types::client_metrics::{ClientApplication, ClientMetricsEnv};
use unleash_types::frontend::EvaluatedToggle;
use unleash_yggdrasil::{Context, EngineState};
use utoipa::ToSchema;

pub type EdgeJsonResult<T> = Result<Json<T>, EdgeError>;
pub type EdgeResult<T> = Result<T, EdgeError>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum TokenType {
    Frontend,
    Client,
    Admin,
}

#[derive(Clone, Debug)]
pub enum ClientFeaturesResponse {
    NoUpdate(EntityTag),
    Updated(ClientFeatures, Option<EntityTag>),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub enum TokenValidationStatus {
    Invalid,
    Unknown,
    Validated,
}

impl Default for TokenValidationStatus {
    fn default() -> Self {
        TokenValidationStatus::Unknown
    }
}

#[derive(Clone, Debug)]
pub struct ClientFeaturesRequest {
    pub api_key: String,
    pub etag: Option<EntityTag>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ValidateTokensRequest {
    pub tokens: Vec<String>,
}

impl ClientFeaturesRequest {
    pub fn new(api_key: String, etag: Option<String>) -> Self {
        Self {
            api_key,
            etag: etag.map(EntityTag::new_weak),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, ToSchema)]
#[cfg_attr(test, derive(Default))]
#[serde(rename_all = "camelCase")]
pub struct EdgeToken {
    pub token: String,
    #[serde(rename = "type")]
    pub token_type: Option<TokenType>,
    pub environment: Option<String>,
    pub projects: Vec<String>,
    #[serde(default = "valid_status")]
    pub status: TokenValidationStatus,
}

fn valid_status() -> TokenValidationStatus {
    TokenValidationStatus::Validated
}

impl PartialEq for EdgeToken {
    fn eq(&self, other: &EdgeToken) -> bool {
        self.token == other.token
    }
}

impl Hash for EdgeToken {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.token.hash(state);
    }
}

impl EdgeToken {
    pub fn no_project_or_environment(s: &str) -> Self {
        EdgeToken {
            token: s.into(),
            token_type: None,
            environment: None,
            projects: vec![],
            status: TokenValidationStatus::default(),
        }
    }

    pub fn subsumes(&self, other: &EdgeToken) -> bool {
        return self.token_type == other.token_type
            && self.environment == other.environment
            && (self.projects.contains(&"*".into())
                || (self.projects.len() >= other.projects.len()
                    && other.projects.iter().all(|p| self.projects.contains(p))));
    }
}

impl FromRequest for EdgeToken {
    type Error = EdgeError;
    type Future = Ready<EdgeResult<Self>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        let value = req.headers().get("Authorization");
        if let Some(data_mode) = req.app_data::<Data<EdgeMode>>() {
            let mode = data_mode.clone().into_inner();
            let key = match *mode {
                EdgeMode::Offline(_) => match value {
                    Some(v) => match v.to_str() {
                        Ok(value) => Ok(EdgeToken::offline_token(value)),
                        Err(_) => Err(EdgeError::AuthorizationDenied),
                    },
                    None => Err(EdgeError::AuthorizationDenied),
                },
                EdgeMode::Edge(_) => match value {
                    Some(v) => EdgeToken::try_from(v.clone()),
                    None => Err(EdgeError::AuthorizationDenied),
                },
            };
            ready(key)
        } else {
            let key = match value {
                Some(v) => EdgeToken::try_from(v.clone()),
                None => Err(EdgeError::AuthorizationDenied),
            };
            ready(key)
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
                    token: s.into(),
                    status: TokenValidationStatus::Unknown,
                })
            } else {
                Err(EdgeError::TokenParseError)
            }
        } else {
            Err(EdgeError::TokenParseError)
        }
    }
}

impl EdgeToken {
    pub fn offline_token(s: &str) -> Self {
        let mut token = EdgeToken::try_from(s.to_string())
            .ok()
            .unwrap_or_else(|| EdgeToken::no_project_or_environment(s));
        token.status = TokenValidationStatus::Validated;
        token
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct TokenStrings {
    pub tokens: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct ValidatedTokens {
    pub tokens: Vec<EdgeToken>,
}

#[async_trait]
pub trait FeatureSource {
    async fn get_client_features(&self, token: &EdgeToken) -> EdgeResult<ClientFeatures>;

    async fn resolve_computed_toggles(
        &self,
        edge_token: &EdgeToken,
        context: &Context,
        include_disabled: bool,
    ) -> EdgeResult<Vec<EvaluatedToggle>>;
}

#[async_trait]
pub trait TokenSource {
    async fn get_tokens(&self) -> EdgeResult<Vec<EdgeToken>>;
    async fn get_valid_tokens(&self) -> EdgeResult<Vec<EdgeToken>>;
    async fn get_token(&self, secret: String) -> EdgeResult<Option<EdgeToken>>;
    async fn filter_valid_tokens(&self, tokens: Vec<String>) -> EdgeResult<Vec<EdgeToken>>;
    async fn get_tokens_due_for_refresh(&self) -> EdgeResult<Vec<TokenRefresh>>;
}

#[derive(Clone, Deserialize, Serialize)]
pub struct TokenRefresh {
    pub token: EdgeToken,
    #[serde(
        deserialize_with = "deserialize_entity_tag",
        serialize_with = "serialize_entity_tag"
    )]
    pub etag: Option<EntityTag>,
    pub last_refreshed: Option<DateTime<Utc>>,
    pub last_check: Option<DateTime<Utc>>,
}

impl TokenRefresh {
    pub fn new(token: EdgeToken) -> Self {
        Self {
            token,
            etag: None,
            last_refreshed: None,
            last_check: None,
        }
    }
}

impl fmt::Debug for TokenRefresh {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("FeatureRefresh")
            .field("token", &"***")
            .field("etag", &self.etag)
            .field("last_refreshed", &self.last_refreshed)
            .field("last_check", &self.last_check)
            .finish()
    }
}
use dashmap::DashMap;
#[derive(Clone, Default)]
pub struct CacheHolder {
    pub token_cache: Arc<DashMap<String, EdgeToken>>,
    pub features_cache: Arc<DashMap<String, ClientFeatures>>,
    pub engine_cache: Arc<DashMap<String, EngineState>>,
}

fn deserialize_entity_tag<'de, D>(deserializer: D) -> Result<Option<EntityTag>, D::Error>
where
    D: Deserializer<'de>,
{
    let s: Option<String> = Option::deserialize(deserializer)?;

    s.map(|s| EntityTag::from_str(&s).map_err(serde::de::Error::custom))
        .transpose()
}

fn serialize_entity_tag<S>(etag: &Option<EntityTag>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let s = etag.as_ref().map(|e| e.to_string());
    serializer.serialize_some(&s)
}

pub fn into_entity_tag(client_features: ClientFeatures) -> Option<EntityTag> {
    client_features.xx3_hash().ok().map(EntityTag::new_weak)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BatchMetricsRequest {
    pub api_key: String,
    pub body: BatchMetricsRequestBody,
}

#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct BatchMetricsRequestBody {
    pub applications: Vec<ClientApplication>,
    pub metrics: Vec<ClientMetricsEnv>,
}

#[async_trait]
pub trait TokenValidator {
    /// Will validate upstream, and add tokens with status from upstream to token cache.
    /// Will block until verified with upstream
    async fn register_tokens(&mut self, tokens: Vec<String>) -> EdgeResult<Vec<EdgeToken>>;
}

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

    fn test_str(token: &str) -> EdgeToken {
        EdgeToken::from_str(
            &(token.to_owned() + ".614a75cf68bef8703aa1bd8304938a81ec871f86ea40c975468eabd6"),
        )
        .unwrap()
    }

    fn test_token(env: Option<&str>, projects: Vec<&str>) -> EdgeToken {
        EdgeToken {
            environment: env.map(|env| env.into()),
            projects: projects.into_iter().map(|p| p.into()).collect(),
            ..EdgeToken::default()
        }
    }

    #[test_case("demo-app:production.614a75cf68bef8703aa1bd8304938a81ec871f86ea40c975468eabd6"; "demo token with project and environment")]
    #[test_case("*:default.5fa5ac2580c7094abf0d87c68b1eeb54bdc485014aef40f9fcb0673b"; "demo token with access to all projects and default environment")]
    fn edge_token_from_string(token: &str) {
        let parsed_token = EdgeToken::from_str(token);
        match parsed_token {
            Ok(t) => {
                assert_eq!(t.token, token);
            }
            Err(e) => {
                warn!("{}", e);
                panic!("Could not parse token");
            }
        }
    }

    #[test_case("943ca9171e2c884c545c5d82417a655fb77cec970cc3b78a8ff87f4406b495d0"; "old java client token")]
    #[test_case("secret-123"; "old example proxy token")]
    fn offline_token_from_string(token: &str) {
        let offline_token = EdgeToken::offline_token(token);
        assert_eq!(offline_token.environment, None);
        assert!(offline_token.projects.is_empty());
    }

    #[test_case(
        "demo-app:production",
        "demo-app:production"
        => true
    ; "idempotency")]
    #[test_case(
        "aproject:production",
        "another:production"
        => false
    ; "project mismatch")]
    #[test_case(
        "demo-app:development",
        "demo-app:production"
        => false
    ; "environment mismatch")]
    #[test_case(
        "*:production",
        "demo-app:production"
        => true
    ; "* subsumes a project token")]
    fn edge_token_subsumes_edge_token(token1: &str, token2: &str) -> bool {
        let t1 = test_str(token1);
        let t2 = test_str(token2);
        t1.subsumes(&t2)
    }

    #[test]
    fn edge_token_unrelated_by_subsume() {
        let t1 = test_str("demo-app:production");
        let t2 = test_str("another:production");
        assert!(!t1.subsumes(&t2));
        assert!(!t2.subsumes(&t1));
    }

    #[test]
    fn edge_token_does_not_subsume_if_projects_is_subset_of_other_tokens_project() {
        let token1 = test_token(None, vec!["p1", "p2"]);

        let token2 = test_token(None, vec!["p1"]);

        assert!(token1.subsumes(&token2));
        assert!(!token2.subsumes(&token1));
    }
}
