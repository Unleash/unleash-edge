use std::collections::HashSet;
use std::future::{Ready, ready};
use std::str::FromStr;

use actix_web::FromRequest;
use actix_web::HttpRequest;
use actix_web::dev::Payload;
use actix_web::http::header::HeaderValue;
use actix_web::web::Data;
use clap::Args;
use dashmap::DashMap;

use crate::EdgeToken;
use crate::TokenRefresh;
use crate::TokenValidationStatus;
use crate::cli::EdgeMode;
use crate::error::EdgeError;
use crate::{EdgeResult, TokenType};

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
    let mut unique_keys = HashSet::new();

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

pub fn cache_key(token: &EdgeToken) -> String {
    token
        .environment
        .clone()
        .unwrap_or_else(|| token.token.clone())
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
}
#[derive(Args, Debug, Clone)]
pub struct TokenHeader {
    /// Token header to use for edge authorization.
    #[clap(long, env, global = true, default_value = "Authorization")]
    pub token_header: String,
}

impl FromStr for TokenHeader {
    type Err = clap::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let token_header = s.to_owned();
        Ok(TokenHeader { token_header })
    }
}
impl FromRequest for EdgeToken {
    type Error = EdgeError;
    type Future = Ready<EdgeResult<Self>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        let token_header = match req.app_data::<Data<TokenHeader>>() {
            Some(data) => data.clone().into_inner().token_header.clone(),
            None => "Authorization".to_string(),
        };
        let value = req.headers().get(token_header);

        if let (Some(value), Some(token_cache)) =
            (value, req.app_data::<Data<DashMap<String, EdgeToken>>>())
        {
            if let Some(token) = token_cache.get(value.to_str().unwrap_or_default()) {
                if token.status == TokenValidationStatus::Trusted {
                    return ready(Ok(token.value().clone()));
                }
            }
        }

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
                _ => unreachable!(),
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
        EdgeToken::from_trimmed_str(s.trim())
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    use actix_http::header::AUTHORIZATION;
    use actix_web::{
        App,
        test::{self as actix_test, TestRequest},
        web::Data,
    };
    use actix_web::{HttpResponse, web};
    use dashmap::DashMap;
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
        .map(|t| TokenRefresh::new(t, None))
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
        .map(|t| TokenRefresh::new(t, None))
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
        .map(|t| TokenRefresh::new(t, None))
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
        .map(|t| TokenRefresh::new(t, None))
        .collect();

        let expected = vec![
            test_token(Some("p1p2_env"), Some("env"), vec!["p1", "p2"]),
            test_token(Some("p2p3_someenv"), Some("env"), vec!["p2", "p3"]),
            test_token(Some("wildcard_noenv"), None, vec!["*"]),
        ];

        let actual: Vec<EdgeToken> = simplify(&tokens).iter().map(|x| x.token.clone()).collect();

        assert_eq!(actual, expected);
    }

    #[test]
    fn test_case_5_when_two_tokens_share_environments_and_products_we_return_only_the_first() {
        let tokens: Vec<TokenRefresh> = vec![
            test_token(Some("abcdefghijklmnopqrst"), Some("development"), vec!["*"]),
            test_token(Some("tsrqponmlkjihgfedcba"), Some("development"), vec!["*"]),
        ]
        .into_iter()
        .map(|t| TokenRefresh::new(t, None))
        .collect();

        let expected = vec![test_token(
            Some("abcdefghijklmnopqrst"),
            Some("development"),
            vec!["*"],
        )];

        let actual: Vec<EdgeToken> = simplify(&tokens).iter().map(|x| x.token.clone()).collect();

        assert_eq!(actual, expected);
    }

    #[test]
    fn test_project_tokens() {
        let df_projects: Vec<TokenRefresh> = vec![
            test_token(Some("my secret"), Some("development"), vec!["df-web"]),
            test_token(
                Some("my other secret"),
                Some("development"),
                vec!["df-platform"],
            ),
        ]
        .into_iter()
        .map(|t| TokenRefresh::new(t, None))
        .collect();
        let actual: Vec<EdgeToken> = simplify(&df_projects)
            .iter()
            .map(|x| x.token.clone())
            .collect();
        assert_eq!(actual.len(), 2);
    }
    #[test]
    fn test_single_project_token_is_covered_by_wildcard() {
        let self_token = EdgeToken {
            projects: vec!["*".into()],
            environment: Some("development".into()),
            ..Default::default()
        };

        let other_token = EdgeToken {
            projects: vec!["A".into()],
            environment: Some("development".into()),
            ..Default::default()
        };

        let is_covered =
            self_token.same_environment_and_broader_or_equal_project_access(&other_token);
        assert!(is_covered);
    }

    #[test]
    fn test_multi_project_token_is_covered_by_wildcard() {
        let self_token = EdgeToken {
            projects: vec!["*".into()],
            environment: Some("development".into()),
            ..Default::default()
        };

        let other_token = EdgeToken {
            projects: vec!["A".into(), "B".into()],
            environment: Some("development".into()),
            ..Default::default()
        };

        let is_covered =
            self_token.same_environment_and_broader_or_equal_project_access(&other_token);
        assert!(is_covered);
    }

    #[test]
    fn test_multi_project_tokens_cover_each_other() {
        let self_token = EdgeToken {
            projects: vec!["A".into(), "B".into()],
            environment: Some("development".into()),
            ..Default::default()
        };

        let fe_token = EdgeToken {
            projects: vec!["A".into()],
            environment: Some("development".into()),
            token_type: Some(TokenType::Frontend),
            ..Default::default()
        };

        let is_covered = self_token.same_environment_and_broader_or_equal_project_access(&fe_token);
        assert!(is_covered);
    }

    #[test]
    fn test_multi_project_tokens_do_not_cover_each_other_when_they_do_not_overlap() {
        let self_token = EdgeToken {
            projects: vec!["A".into(), "B".into()],
            environment: Some("development".into()),
            ..Default::default()
        };

        let fe_token = EdgeToken {
            projects: vec!["A".into(), "C".into()],
            environment: Some("development".into()),
            token_type: Some(TokenType::Frontend),
            ..Default::default()
        };

        let is_covered = self_token.same_environment_and_broader_or_equal_project_access(&fe_token);
        assert!(!is_covered);
    }

    #[test]
    fn leading_or_trailing_whitespace_gets_trimmed_away_when_constructing_token() {
        let some_token = "*:development.somesecretstring";
        let some_token_with_leading_whitespace = "  *:development.somesecretstring";
        let some_token_with_trailing_whitespace = "*:development.somesecretstring    ";
        let some_token_with_leading_and_trailing_whitespace =
            "    *:development.somesecretstring     ";
        let token = EdgeToken::from_str(some_token).expect("Could not parse token");
        let token1 =
            EdgeToken::from_str(some_token_with_leading_whitespace).expect("Could not parse token");
        let token2 = EdgeToken::from_str(some_token_with_trailing_whitespace)
            .expect("Could not parse token");
        let token3 = EdgeToken::from_str(some_token_with_leading_and_trailing_whitespace)
            .expect("Could not parse token");
        assert_eq!(token, token1);
        assert_eq!(token1, token2);
        assert_eq!(token2, token3);
    }

    #[tokio::test]
    async fn invalid_aka_legacy_tokens_can_be_parsed_from_request_if_already_in_cache() {
        let token_cache = DashMap::<String, EdgeToken>::new();

        token_cache.insert(
            "legacy-123".to_string(),
            EdgeToken {
                token_type: Some(TokenType::Frontend),
                status: TokenValidationStatus::Trusted,
                environment: Some("some-weird-environment-I-made-up".to_string()),
                ..Default::default()
            },
        );
        let app = actix_test::init_service(App::new().app_data(Data::new(token_cache)).route(
            "/",
            web::get().to(|token: EdgeToken| async move {
                HttpResponse::Ok().body(token.environment.unwrap())
            }),
        ))
        .await;

        let req = TestRequest::get()
            .uri("/")
            .insert_header((AUTHORIZATION, "legacy-123"))
            .to_request();

        let resp = actix_test::call_and_read_body(&app, req).await;
        assert_eq!(resp, "some-weird-environment-I-made-up");
    }

    #[tokio::test]
    async fn non_trusted_tokens_cannot_be_retrieved_from_cache() {
        let token_cache = DashMap::<String, EdgeToken>::new();

        token_cache.insert(
            "legacy-123".to_string(),
            EdgeToken {
                token_type: Some(TokenType::Frontend),
                status: TokenValidationStatus::Validated,
                environment: Some("some-weird-environment-I-made-up".to_string()),
                ..Default::default()
            },
        );
        let app = actix_test::init_service(App::new().app_data(Data::new(token_cache)).route(
            "/",
            web::get().to(|token: EdgeToken| async move {
                HttpResponse::Ok().body(token.environment.unwrap())
            }),
        ))
        .await;

        let req = TestRequest::get()
            .uri("/")
            .insert_header((AUTHORIZATION, "legacy-123"))
            .to_request();

        let resp = actix_test::call_and_read_body(&app, req).await;
        assert_eq!(
            resp,
            "{\"explanation\":\"Edge could not parse token: legacy-123\"}"
        );
    }

    #[test]
    fn parses_valid_token_as_trusted() {
        let input = "*:development.some-hash-blob";
        let parsed = parse_trusted_token_pair(input).expect("should parse");

        assert_eq!(parsed.0, input);
        assert_eq!(parsed.1.token, input);
        assert_eq!(parsed.1.status, TokenValidationStatus::Trusted);
        assert_eq!(parsed.1.token_type, Some(TokenType::Frontend));
    }

    #[test]
    fn parses_legacy_token_correctly() {
        let input = "mysecret@development";
        let (raw, token) = parse_trusted_token_pair(input).expect("should parse legacy token");

        assert_eq!(raw, "mysecret");
        assert_eq!(token.token, "*.development:mysecret");
        assert_eq!(token.environment.as_deref(), Some("development"));
        assert_eq!(token.projects, vec!["*"]);
        assert_eq!(token.token_type, Some(TokenType::Frontend));
        assert_eq!(token.status, TokenValidationStatus::Trusted);
    }

    #[test]
    fn fails_on_bad_legacy_token_format() {
        let input = "missingatseparator";
        let err = parse_trusted_token_pair(input).expect_err("should fail");

        match err {
            EdgeError::TokenParseError(msg) => {
                assert!(msg.contains("must be {string}@{environment}"));
            }
            _ => panic!("wrong error type"),
        }
    }
}
