use std::{
    future::{ready, Ready},
    str::FromStr,
};

use actix_web::{
    dev::Payload,
    http::header::HeaderValue,
    web::{Data, Json},
    FromRequest, HttpRequest,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::warn;
use unleash_types::client_features::ClientFeatures;

use crate::error::EdgeError;

pub type EdgeJsonResult<T> = Result<Json<T>, EdgeError>;
pub type EdgeResult<T> = Result<T, EdgeError>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TokenType {
    Frontend,
    Client,
    Admin,
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
                if token_provider.secret_is_valid(&client_token.secret) {
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
    fn get_client_features(&self, token: EdgeToken) -> ClientFeatures;
}

pub trait TokenProvider {
    fn get_known_tokens(&self) -> Vec<EdgeToken>;
    fn secret_is_valid(&self, secret: &str) -> bool;
    fn token_details(&self, secret: String) -> Option<EdgeToken>;
}

pub trait EdgeProvider: FeaturesProvider + TokenProvider {}

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
