use crate::errors::EdgeError;
use crate::{ClientTokenRequest, EdgeResult, TokenRefresh, TokenType, TokenValidationStatus};
use axum::http::HeaderValue;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fmt::{Debug, Formatter};
use std::hash::{Hash, Hasher};
use std::str::FromStr;
use ahash::HashSet;
use utoipa::ToSchema;

#[derive(Clone, Default, Serialize, Deserialize, Eq, ToSchema)]
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

pub fn cache_key(token: &EdgeToken) -> String {
    token
        .environment
        .clone()
        .unwrap_or_else(|| token.token.clone())
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
        EdgeToken::from_trimmed_str(s.trim())
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
        self.token_type == other.token_type
            && self.same_environment_and_broader_or_equal_project_access(other)
    }

    pub fn same_environment_and_broader_or_equal_project_access(&self, other: &EdgeToken) -> bool {
        self.environment == other.environment
            && (self.projects.contains(&"*".into())
                || (self.projects.len() >= other.projects.len()
                    && other.projects.iter().all(|p| self.projects.contains(p))))
    }
    pub fn offline_token(s: &str) -> Self {
        let mut token = EdgeToken::try_from(s.to_string())
            .ok()
            .unwrap_or_else(|| EdgeToken::no_project_or_environment(s));
        token.status = TokenValidationStatus::Validated;
        token
    }
    pub fn from_trimmed_str(s: &str) -> Result<Self, EdgeError> {
        if s.contains(':') && s.contains('.') {
            let token_parts: Vec<String> = s.split(':').take(2).map(|s| s.to_string()).collect();
            let token_projects = if let Some(projects) = token_parts.first() {
                if projects == "[]" {
                    vec![]
                } else {
                    vec![projects.clone()]
                }
            } else {
                return Err(EdgeError::TokenParseError(s.into()));
            };
            if let Some(env_and_key) = token_parts.get(1) {
                let e_a_k: Vec<String> = env_and_key
                    .split('.')
                    .take(2)
                    .map(|s| s.to_string())
                    .collect();
                if e_a_k.len() != 2 {
                    return Err(EdgeError::TokenParseError(s.into()));
                }
                Ok(EdgeToken {
                    environment: e_a_k.first().cloned(),
                    projects: token_projects,
                    token_type: None,
                    token: s.into(),
                    status: TokenValidationStatus::Unknown,
                })
            } else {
                Err(EdgeError::TokenParseError(s.into()))
            }
        } else {
            Err(EdgeError::TokenParseError(s.into()))
        }
    }
}

pub fn parse_trusted_token_pair(token_string: &str) -> EdgeResult<(String, EdgeToken)> {
    match EdgeToken::from_str(token_string) {
        Ok(token) => Ok((
            token_string.into(),
            EdgeToken {
                token: token.token.clone(),
                environment: token.environment.clone(),
                projects: token.projects.clone(),
                token_type: Some(TokenType::Frontend),
                status: TokenValidationStatus::Trusted,
            },
        )),
        Err(EdgeError::TokenParseError(_)) => parse_legacy_token(token_string),
        Err(e) => Err(e),
    }
}

fn parse_legacy_token(token_string: &str) -> EdgeResult<(String, EdgeToken)> {
    let parts: Vec<&str> = token_string.split('@').collect();
    if parts.len() != 2 {
        Err(EdgeError::TokenParseError("Trusted tokens must either match the existing Unleash token format or they must be {string}@{environment}".into()))
    } else {
        Ok((
            parts[0].into(),
            EdgeToken {
                token: format!("*.{}:{}", parts[1], parts[0]),
                environment: Some(parts[1].to_string()),
                projects: vec!["*".into()],
                token_type: Some(TokenType::Frontend),
                status: TokenValidationStatus::Trusted,
            },
        ))
    }
}

pub fn simplify(tokens: &[TokenRefresh]) -> Vec<TokenRefresh> {
    let uniques = filter_unique_tokens(tokens);
    uniques
        .iter()
        .filter_map(|token| {
            uniques.iter().try_fold(token, |acc, current| {
                if current.token.token != acc.token.token && current.token.subsumes(&acc.token) {
                    None
                } else {
                    Some(acc)
                }
            })
        })
        .cloned()
        .collect()
}

fn filter_unique_tokens(tokens: &[TokenRefresh]) -> Vec<TokenRefresh> {
    let mut unique_tokens = Vec::new();
    let mut unique_keys = HashSet::default();

    for token in tokens {
        let key = (
            token.token.projects.clone(),
            token.token.environment.clone(),
        );
        if !unique_keys.contains(&key) {
            unique_tokens.push(token.clone());
            unique_keys.insert(key);
        }
    }

    unique_tokens
}

pub fn anonymize_token(edge_token: &EdgeToken) -> EdgeToken {
    let mut iterator = edge_token.token.split('.');
    let project_and_environment = iterator.next();
    let maybe_hash = iterator.next();
    match (project_and_environment, maybe_hash) {
        (Some(p_and_e), Some(hash)) => {
            let safe_hash = clean_hash(hash);
            EdgeToken {
                token: format!("{}.{}", p_and_e, safe_hash),
                ..edge_token.clone()
            }
        }
        _ => edge_token.clone(),
    }
}
fn clean_hash(hash: &str) -> String {
    format!(
        "{}****{}",
        &hash[..6].to_string(),
        &hash[hash.len() - 6..].to_string()
    )
}