use std::cmp::min;
use std::fmt;
use std::fmt::{Debug, Display, Formatter};
use std::net::IpAddr;
use std::sync::Arc;
use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    str::FromStr,
};

use actix_web::{http::header::EntityTag, web::Json};
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use shadow_rs::shadow;
use unleash_types::client_features::Context;
use unleash_types::client_features::{ClientFeatures, ClientFeaturesDelta};
use unleash_types::client_metrics::{ClientApplication, ClientMetricsEnv};
use unleash_yggdrasil::EngineState;
use utoipa::{IntoParams, ToSchema};

use crate::error::EdgeError;
use crate::metrics::client_metrics::MetricsKey;

pub type EdgeJsonResult<T> = Result<Json<T>, EdgeError>;
pub type EdgeResult<T> = Result<T, EdgeError>;

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct IncomingContext {
    #[serde(flatten)]
    pub context: Context,

    #[serde(flatten)]
    pub extra_properties: HashMap<String, String>,
}

impl From<IncomingContext> for Context {
    fn from(input: IncomingContext) -> Self {
        let properties = if input.extra_properties.is_empty() {
            input.context.properties
        } else {
            let mut input_properties = input.extra_properties;
            input_properties.extend(input.context.properties.unwrap_or_default());
            Some(input_properties)
        };
        Context {
            properties,
            ..input.context
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PostContext {
    pub context: Option<Context>,
    #[serde(flatten)]
    pub flattened_context: Option<Context>,
    #[serde(flatten)]
    pub extra_properties: HashMap<String, String>,
}

impl From<PostContext> for Context {
    fn from(input: PostContext) -> Self {
        if let Some(context) = input.context {
            context
        } else {
            IncomingContext {
                context: input.flattened_context.unwrap_or_default(),
                extra_properties: input.extra_properties,
            }
            .into()
        }
    }
}

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
#[allow(clippy::large_enum_variant)]
pub enum ClientFeaturesResponse {
    NoUpdate(EntityTag),
    Updated(ClientFeatures, Option<EntityTag>),
}

#[derive(Clone, Debug)]
pub enum ClientFeaturesDeltaResponse {
    NoUpdate(EntityTag),
    Updated(ClientFeaturesDelta, Option<EntityTag>),
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
    pub interval: Option<i64>,
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

    #[cfg(test)]
    pub fn validated_client_token(token: &str) -> Self {
        EdgeToken::from_str(token)
            .map(|mut t| {
                t.status = TokenValidationStatus::Validated;
                t.token_type = Some(TokenType::Client);
                t
            })
            .unwrap()
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
    pub last_feature_count: Option<usize>,
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
            last_feature_count: None,
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
    pub fn successful_refresh(
        &self,
        refresh_interval: &Duration,
        etag: Option<EntityTag>,
        feature_count: usize,
    ) -> Self {
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
            last_feature_count: Some(feature_count),
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
            #[allow(clippy::const_is_empty)]
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

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TokenInfo {
    pub token_refreshes: Vec<TokenRefresh>,
    pub token_validation_status: Vec<EdgeToken>,
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct ClientMetric {
    pub key: MetricsKey,
    pub bucket: ClientMetricsEnv,
}
#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct MetricsInfo {
    pub applications: Vec<ClientApplication>,
    pub metrics: Vec<ClientMetric>,
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::str::FromStr;

    use serde_json::json;
    use test_case::test_case;
    use tracing::warn;
    use unleash_types::client_features::Context;

    use crate::error::EdgeError::EdgeTokenParseError;
    use crate::http::unleash_client::EdgeTokens;
    use crate::types::{EdgeResult, EdgeToken, IncomingContext};

    use super::PostContext;

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

    #[test]
    fn context_conversion_works() {
        let context = Context {
            user_id: Some("user".into()),
            session_id: Some("session".into()),
            environment: Some("env".into()),
            app_name: Some("app".into()),
            current_time: Some("2024-03-12T11:42:46+01:00".into()),
            remote_address: Some("127.0.0.1".into()),
            properties: Some(HashMap::from([("normal property".into(), "normal".into())])),
        };

        let extra_properties =
            HashMap::from([(String::from("top-level property"), String::from("top"))]);

        let incoming_context = IncomingContext {
            context: context.clone(),
            extra_properties: extra_properties.clone(),
        };

        let converted: Context = incoming_context.into();
        assert_eq!(converted.user_id, context.user_id);
        assert_eq!(converted.session_id, context.session_id);
        assert_eq!(converted.environment, context.environment);
        assert_eq!(converted.app_name, context.app_name);
        assert_eq!(converted.current_time, context.current_time);
        assert_eq!(converted.remote_address, context.remote_address);
        assert_eq!(
            converted.properties,
            Some(HashMap::from([
                ("normal property".into(), "normal".into()),
                ("top-level property".into(), "top".into())
            ]))
        );
    }

    #[test]
    fn context_conversion_properties_level_properties_take_precedence_over_top_level() {
        let context = Context {
            properties: Some(HashMap::from([(
                "duplicated property".into(),
                "lower".into(),
            )])),
            ..Default::default()
        };

        let extra_properties =
            HashMap::from([(String::from("duplicated property"), String::from("upper"))]);

        let incoming_context = IncomingContext {
            context: context.clone(),
            extra_properties: extra_properties.clone(),
        };

        let converted: Context = incoming_context.into();
        assert_eq!(
            converted.properties,
            Some(HashMap::from([(
                "duplicated property".into(),
                "lower".into()
            ),]))
        );
    }

    #[test]
    fn context_conversion_if_there_are_no_extra_properties_the_properties_hash_map_is_none() {
        let context = Context {
            properties: None,
            ..Default::default()
        };

        let extra_properties = HashMap::new();

        let incoming_context = IncomingContext {
            context: context.clone(),
            extra_properties: extra_properties.clone(),
        };

        let converted: Context = incoming_context.into();
        assert_eq!(converted.properties, None);
    }

    #[test]
    fn completely_flat_json_parses_to_a_context() {
        let json = json!(
            {
                "userId": "7",
                "flat": "endsUpInProps",
                "invalidProperty": "alsoEndsUpInProps"
            }
        );

        let post_context: PostContext = serde_json::from_value(json).unwrap();
        let parsed_context: Context = post_context.into();

        assert_eq!(parsed_context.user_id, Some("7".into()));
        assert_eq!(
            parsed_context.properties,
            Some(HashMap::from([
                ("flat".into(), "endsUpInProps".into()),
                ("invalidProperty".into(), "alsoEndsUpInProps".into())
            ]))
        );
    }

    #[test]
    fn post_context_root_level_properties_are_ignored_if_context_property_is_set() {
        let json = json!(
            {
                "context": {
                    "userId":"7",
                },
                "invalidProperty": "thisNeverGoesAnywhere",
                "anotherInvalidProperty": "alsoGoesNoWhere"
            }
        );

        let post_context: PostContext = serde_json::from_value(json).unwrap();
        let parsed_context: Context = post_context.into();
        assert_eq!(parsed_context.properties, None);

        assert_eq!(parsed_context.user_id, Some("7".into()));
    }

    #[test]
    fn post_context_properties_are_taken_from_nested_context_object_but_root_levels_are_ignored() {
        let json = json!(
            {
                "context": {
                    "userId":"7",
                    "properties": {
                        "nested": "nestedValue"
                    }
                },
                "invalidProperty": "thisNeverGoesAnywhere"
            }
        );

        let post_context: PostContext = serde_json::from_value(json).unwrap();
        let parsed_context: Context = post_context.into();
        assert_eq!(
            parsed_context.properties,
            Some(HashMap::from([("nested".into(), "nestedValue".into()),]))
        );

        assert_eq!(parsed_context.user_id, Some("7".into()));
    }

    #[test]
    fn post_context_properties_are_taken_from_nested_context_object_but_custom_properties_on_context_are_ignored(
    ) {
        let json = json!(
            {
                "context": {
                    "userId":"7",
                    "howDidYouGetHere": "I dunno bro",
                    "properties": {
                        "nested": "nestedValue"
                    }
                },
                "flat": "endsUpInProps",
                "invalidProperty": "thisNeverGoesAnywhere"
            }
        );

        let post_context: PostContext = serde_json::from_value(json).unwrap();
        let parsed_context: Context = post_context.into();
        assert_eq!(
            parsed_context.properties,
            Some(HashMap::from([("nested".into(), "nestedValue".into()),]))
        );

        assert_eq!(parsed_context.user_id, Some("7".into()));
    }
}
