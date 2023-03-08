use actix_web::dev::Payload;
use actix_web::http::header::HeaderValue;
use actix_web::web::Data;
use actix_web::FromRequest;
use actix_web::HttpRequest;
use dashmap::DashMap;
use std::future::{ready, Ready};
use std::str::FromStr;

use crate::cli::EdgeMode;
use crate::error::EdgeError;
use crate::types::EdgeResult;
use crate::types::EdgeToken;
use crate::types::TokenRefresh;
use crate::types::TokenValidationStatus;

pub(crate) fn simplify(tokens: &[TokenRefresh]) -> Vec<&TokenRefresh> {
    tokens
        .iter()
        .filter_map(|token| {
            tokens.iter().fold(Some(token), |acc, current| {
                acc.and_then(|lead| {
                    if current.token.token != lead.token.token
                        && current.token.subsumes(&lead.token)
                    {
                        None
                    } else {
                        Some(lead)
                    }
                })
            })
        })
        .collect()
}

pub(crate) fn cache_key(token: EdgeToken) -> String {
    token.environment.unwrap_or(token.token)
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
            let key = match key {
                Ok(k) => {
                    let token_cache = req.app_data::<Data<DashMap<String, EdgeToken>>>();
                    if let Some(cache) = token_cache {
                        cache
                            .get(&k.token)
                            .map(|e| e.value().clone())
                            .ok_or(EdgeError::AuthorizationDenied)
                    } else {
                        Ok(k)
                    }
                }
                Err(e) => Err(e),
            };

            ready(key)
        } else {
            let key = match value {
                Some(v) => EdgeToken::try_from(v.clone()).and_then(|k| {
                    let token_cache = req.app_data::<Data<DashMap<String, EdgeToken>>>();
                    if let Some(cache) = token_cache {
                        cache
                            .get(&k.token)
                            .map(|e| e.value().clone())
                            .ok_or(EdgeError::AuthorizationDenied)
                    } else {
                        Ok(k)
                    }
                }),
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
#[cfg(test)]
mod tests {
    use crate::{
        tokens::simplify,
        types::{EdgeToken, TokenRefresh},
    };
    use ulid::Ulid;

    fn test_token(token: Option<&str>, env: Option<&str>, projects: Vec<&str>) -> EdgeToken {
        EdgeToken {
            token: token
                .map(|s| s.into())
                .unwrap_or_else(|| Ulid::new().to_string()),
            environment: env.map(|env| env.into()),
            projects: projects.into_iter().map(|p| p.into()).collect(),
            ..EdgeToken::default()
        }
    }

    #[test]
    fn test_case_1_token_with_two_projects_subsumes_tokens_having_individually_each_token() {
        let tokens: Vec<TokenRefresh> = vec![
            test_token(Some("twoprojects"), None, vec!["p1", "p2"]),
            test_token(Some("p1project"), None, vec!["p1"]),
            test_token(Some("p1project2"), None, vec!["p1"]),
        ]
        .into_iter()
        .map(TokenRefresh::new)
        .collect();

        let expected = vec![test_token(Some("twoprojects"), None, vec!["p1", "p2"])];
        let actual: Vec<EdgeToken> = simplify(&tokens).iter().map(|x| x.token.clone()).collect();

        assert_eq!(actual, expected);
    }

    #[test]
    fn test_case_2_when_two_environments_are_different_we_have_at_least_two_tokens() {
        let tokens: Vec<TokenRefresh> = vec![
            test_token(Some("env1_twoprojects"), Some("env1"), vec!["p1", "p2"]),
            test_token(Some("env1_p1"), Some("env1"), vec!["p1"]),
            test_token(Some("p1"), None, vec!["p1"]),
        ]
        .into_iter()
        .map(TokenRefresh::new)
        .collect();

        let expected = vec![
            test_token(Some("env1_twoprojects"), Some("env1"), vec!["p1", "p2"]),
            test_token(Some("p1"), None, vec!["p1"]),
        ];

        let actual: Vec<EdgeToken> = simplify(&tokens).iter().map(|x| x.token.clone()).collect();

        assert_eq!(actual, expected);
    }

    #[test]
    fn test_case_3_star_token_subsumes_all_tokens() {
        let tokens: Vec<TokenRefresh> = vec![
            test_token(Some("p1"), None, vec!["p1"]),
            test_token(Some("wildcard"), None, vec!["*"]),
            test_token(Some("p1_and_p2"), None, vec!["p1", "p2"]),
        ]
        .into_iter()
        .map(TokenRefresh::new)
        .collect();
        let expected = vec![test_token(Some("wildcard"), None, vec!["*"])];

        let actual: Vec<EdgeToken> = simplify(&tokens).iter().map(|x| x.token.clone()).collect();

        assert_eq!(actual, expected);
    }

    #[test]
    fn test_case_4_when_a_project_is_shared_between_two_tokens_we_simplify_as_much_as_we_can() {
        let tokens: Vec<TokenRefresh> = vec![
            test_token(Some("p1p2_noenv"), None, vec!["p1", "p2"]),
            test_token(Some("p1p2_env"), Some("env"), vec!["p1", "p2"]),
            test_token(Some("p1_noenv"), None, vec!["p1"]),
            test_token(Some("p2p3_someenv"), Some("env"), vec!["p2", "p3"]),
            test_token(Some("wildcard_noenv"), None, vec!["*"]),
            test_token(Some("p1_someenv"), Some("env"), vec!["p1"]),
            test_token(Some("p3_noenv"), None, vec!["p3"]),
            test_token(Some("p2_someenv"), Some("env"), vec!["p2"]),
        ]
        .into_iter()
        .map(TokenRefresh::new)
        .collect();

        let expected = vec![
            test_token(Some("p1p2_env"), Some("env"), vec!["p1", "p2"]),
            test_token(Some("p2p3_someenv"), Some("env"), vec!["p2", "p3"]),
            test_token(Some("wildcard_noenv"), None, vec!["*"]),
        ];

        let actual: Vec<EdgeToken> = simplify(&tokens).iter().map(|x| x.token.clone()).collect();

        assert_eq!(actual, expected);
    }
}
