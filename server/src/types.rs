use std::cmp::min;
use std::fmt;
use std::fmt::{Debug, Display, Formatter};
use std::net::IpAddr;

use std::sync::Arc;
use std::{
    hash::{Hash, Hasher},
    str::FromStr,
};

use crate::error::EdgeError;
use actix_web::{http::header::EntityTag, web::Json};
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use shadow_rs::shadow;
use unleash_types::client_features::ClientFeatures;
use unleash_types::client_metrics::{ClientApplication, ClientMetricsEnv};
use unleash_yggdrasil::EngineState;
use utoipa::{IntoParams, ToSchema};

pub type EdgeJsonResult<T> = Result<Json<T>, EdgeError>;
pub type EdgeResult<T> = Result<T, EdgeError>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum TokenType {
    #[serde(alias = "FRONTEND")]
    Frontend,
    #[serde(alias = "CLIENT")]
    Client,
    #[serde(alias = "ADMIN")]
    Admin,
    Invalid,
}

#[derive(Clone, Debug)]
pub enum ClientFeaturesResponse {
    NoUpdate(EntityTag),
    Updated(ClientFeatures, Option<EntityTag>),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Default, Deserialize, utoipa::ToSchema)]
pub enum TokenValidationStatus {
    Invalid,
    #[default]
    Unknown,
    Validated,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum Status {
    Ok,
    NotOk,
    NotReady,
    Ready,
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

#[derive(Clone, Serialize, Deserialize, Eq, ToSchema)]
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

impl Debug for EdgeToken {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("EdgeToken")
            .field(
                "token",
                &format!(
                    "{}.[redacted]",
                    &self
                        .token
                        .chars()
                        .take_while(|p| p != &'.')
                        .collect::<String>()
                ),
            )
            .field("token_type", &self.token_type)
            .field("environment", &self.environment)
            .field("projects", &self.projects)
            .field("status", &self.status)
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct ServiceAccountToken {
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClientTokenResponse {
    pub secret: String,
    pub token_name: String,
    #[serde(rename = "type")]
    pub token_type: Option<TokenType>,
    pub environment: Option<String>,
    pub project: Option<String>,
    pub projects: Vec<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: Option<DateTime<Utc>>,
    pub seen_at: Option<DateTime<Utc>>,
    pub alias: Option<String>,
}

impl From<ClientTokenResponse> for EdgeToken {
    fn from(value: ClientTokenResponse) -> Self {
        Self {
            token: value.secret,
            token_type: value.token_type,
            environment: value.environment,
            projects: value.projects,
            status: TokenValidationStatus::Validated,
        }
    }
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
    pub fn to_client_token_request(&self) -> ClientTokenRequest {
        ClientTokenRequest {
            token_name: format!(
                "edge_data_token_{}",
                self.environment.clone().unwrap_or("default".into())
            ),
            token_type: TokenType::Client,
            projects: self.projects.clone(),
            environment: self.environment.clone().unwrap_or("default".into()),
            expires_at: Utc::now() + Duration::weeks(4),
        }
    }
    pub fn admin_token(secret: &str) -> Self {
        Self {
            token: format!("*:*.{}", secret),
            status: TokenValidationStatus::Validated,
            token_type: Some(TokenType::Admin),
            environment: None,
            projects: vec!["*".into()],
        }
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientIp {
    pub ip: IpAddr,
}

impl Display for ClientIp {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.ip)
    }
}

#[derive(Clone, Deserialize, Serialize)]
pub struct TokenRefresh {
    pub token: EdgeToken,
    #[serde(
        deserialize_with = "deserialize_entity_tag",
        serialize_with = "serialize_entity_tag"
    )]
    pub etag: Option<EntityTag>,
    pub next_refresh: Option<DateTime<Utc>>,
    pub last_refreshed: Option<DateTime<Utc>>,
    pub last_check: Option<DateTime<Utc>>,
    pub failure_count: u32,
}

#[derive(Clone, Deserialize, Serialize, Debug)]
pub struct UnleashValidationDetail {
    pub path: Option<String>,
    pub description: Option<String>,
    pub message: Option<String>,
}

#[derive(Clone, Deserialize, Serialize, Debug)]
pub struct UnleashBadRequest {
    pub id: Option<String>,
    pub name: Option<String>,
    pub message: Option<String>,
    pub details: Option<Vec<UnleashValidationDetail>>,
}

impl TokenRefresh {
    pub fn new(token: EdgeToken, etag: Option<EntityTag>) -> Self {
        Self {
            token,
            etag,
            last_refreshed: None,
            last_check: None,
            next_refresh: None,
            failure_count: 0,
        }
    }

    /// Something went wrong (but it was retriable. Increment our failure count and set last_checked and next_refresh
    pub fn backoff(&self, refresh_interval: &Duration) -> Self {
        let failure_count: u32 = min(self.failure_count + 1, 10);
        let now = Utc::now();
        let next_refresh = calculate_next_refresh(now, *refresh_interval, failure_count as u64);
        Self {
            failure_count,
            next_refresh: Some(next_refresh),
            last_check: Some(now),
            ..self.clone()
        }
    }
    /// We successfully talked to upstream, but there was no updates. Update our next_refresh, decrement our failure count and set when we last_checked
    pub fn successful_check(&self, refresh_interval: &Duration) -> Self {
        let failure_count = if self.failure_count > 0 {
            self.failure_count - 1
        } else {
            0
        };
        let now = Utc::now();
        let next_refresh = calculate_next_refresh(now, *refresh_interval, failure_count as u64);
        Self {
            failure_count,
            next_refresh: Some(next_refresh),
            last_check: Some(now),
            ..self.clone()
        }
    }
    /// We successfully talked to upstream. There were updates. Update next_refresh, last_refreshed and last_check, and decrement our failure count
    pub fn successful_refresh(&self, refresh_interval: &Duration, etag: Option<EntityTag>) -> Self {
        let failure_count = if self.failure_count > 0 {
            self.failure_count - 1
        } else {
            0
        };
        let now = Utc::now();
        let next_refresh = calculate_next_refresh(now, *refresh_interval, failure_count as u64);
        Self {
            failure_count,
            next_refresh: Some(next_refresh),
            last_refreshed: Some(now),
            last_check: Some(now),
            etag,
            ..self.clone()
        }
    }
}

fn calculate_next_refresh(
    now: DateTime<Utc>,
    refresh_interval: Duration,
    failure_count: u64,
) -> DateTime<Utc> {
    if failure_count == 0 {
        now + refresh_interval
    } else {
        now + refresh_interval + (refresh_interval * (failure_count.try_into().unwrap_or(0)))
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

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ClientTokenRequest {
    pub token_name: String,
    #[serde(rename = "type")]
    pub token_type: TokenType,
    pub projects: Vec<String>,
    pub environment: String,
    pub expires_at: DateTime<Utc>,
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
pub const EDGE_VERSION: &str = build::PKG_VERSION;
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

#[derive(Clone, Debug, Serialize, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct FeatureFilters {
    pub name_prefix: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TokenInfo {
    pub token_refreshes: Vec<TokenRefresh>,
    pub token_validation_status: Vec<EdgeToken>,
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use crate::error::EdgeError::EdgeTokenParseError;
    use crate::http::unleash_client::EdgeTokens;
    use test_case::test_case;
    use tracing::warn;

    use crate::types::{EdgeResult, EdgeToken};

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

    #[test]
    fn token_type_should_be_case_insensitive() {
        let json = r#"{ "tokens": [{
              "token": "chriswk-test:development.notusedsecret",
              "type": "CLIENT",
              "projects": [
                "chriswk-test"
              ]
            },
            {
              "token": "demo-app:production.notusedsecret",
              "type": "client",
              "projects": [
                "demo-app"
              ]
            }] }"#;
        let tokens: EdgeResult<EdgeTokens> =
            serde_json::from_str(json).map_err(|_| EdgeTokenParseError);
        assert!(tokens.is_ok());
        assert_eq!(tokens.unwrap().tokens.len(), 2);
    }
}
