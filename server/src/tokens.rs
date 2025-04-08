use std::collections::HashSet;
use std::future::{Ready, ready};
use std::str::FromStr;

use actix_web::FromRequest;
use actix_web::HttpRequest;
use actix_web::dev::Payload;
use actix_web::http::header::HeaderValue;
use actix_web::web::Data;

use crate::cli::EdgeMode;
use crate::cli::TokenHeader;
use crate::error::EdgeError;
use crate::types::TokenRefresh;
use crate::types::TokenValidationStatus;
use crate::types::{EdgeResult, Projects};
use crate::types::{EdgeToken, EnvironmentStatus};

pub(crate) fn simplify(tokens: &[TokenRefresh]) -> Vec<TokenRefresh> {
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
        let key = (token.token.projects().clone(), token.token.environment());
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
    match token.environment() {
        EnvironmentStatus::Unknown => token.token.clone(),
        EnvironmentStatus::Known(env) => env,
    }
}

impl EdgeToken {
    pub fn non_validated_token(s: &str) -> Self {
        EdgeToken {
            token: s.into(),
            token_type: None,
            status: TokenValidationStatus::default(),
        }
    }

    pub(crate) fn subsumes(&self, other: &EdgeToken) -> bool {
        self.token_type == other.token_type
            && self.same_environment_and_broader_or_equal_project_access(other)
    }

    pub(crate) fn same_environment_and_broader_or_equal_project_access(
        &self,
        other: &EdgeToken,
    ) -> bool {
        if self.environment() != other.environment() {
            return false;
        }
        if self.is_wildcard() {
            return true;
        }
        let own_projects = self.projects();
        let other_projects = other.projects();
        own_projects.len() >= other.projects().len()
            && other_projects.iter().all(|p| own_projects.contains(p))
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
        Ok(EdgeToken::non_validated_token(s.trim()))
    }
}

impl EdgeToken {
    pub fn offline_token(s: &str) -> Self {
        let mut token = EdgeToken::non_validated_token(s);
        if let Ok((projects, env)) = EdgeToken::try_extract_projects_and_environment_from_str(s) {
            token.status = TokenValidationStatus::Offline(projects, env)
        } else {
            token.status = TokenValidationStatus::OfflineWildcard
        }
        token
    }
    pub fn try_extract_projects_and_environment_from_str(
        s: &str,
    ) -> Result<(Projects, EnvironmentStatus), EdgeError> {
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
                Ok((
                    Projects::new(token_projects),
                    e_a_k
                        .first()
                        .cloned()
                        .map(|env| EnvironmentStatus::Known(env))
                        .unwrap_or(EnvironmentStatus::Unknown),
                ))
            } else {
                Err(EdgeError::TokenParseError(s.into()))
            }
        } else {
            Err(EdgeError::TokenParseError(s.into()))
        }
    }
}
#[cfg(test)]
mod tests {
    use std::str::FromStr;
    use ulid::Ulid;

    use crate::types::{EnvironmentStatus, Projects, TokenValidationStatus};
    use crate::{
        tokens::simplify,
        types::{EdgeToken, TokenRefresh, TokenType},
    };

    fn test_token(token: Option<&str>, env: Option<&str>, projects: Vec<&str>) -> EdgeToken {
        EdgeToken {
            token: token
                .map(|s| s.into())
                .unwrap_or_else(|| Ulid::new().to_string()),
            token_type: Some(TokenType::Client),
            status: TokenValidationStatus::Validated(
                Projects::new(projects.iter().map(|i| i.to_string()).collect()),
                env.map(|env| EnvironmentStatus::Known(env.into()))
                    .unwrap_or(EnvironmentStatus::Unknown),
            ),
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
        let env_status = EnvironmentStatus::Known("development".into());
        let self_token = EdgeToken {
            status: TokenValidationStatus::Validated(
                Projects::wildcard_project(),
                env_status.clone(),
            ),
            ..EdgeToken::default()
        };

        let other_token = EdgeToken {
            status: TokenValidationStatus::Validated(Projects::single("A"), env_status.clone()),
            ..Default::default()
        };

        let is_covered =
            self_token.same_environment_and_broader_or_equal_project_access(&other_token);
        assert!(is_covered);
    }

    #[test]
    fn test_multi_project_token_is_covered_by_wildcard() {
        let env_status = EnvironmentStatus::Known("development".into());
        let self_token = EdgeToken {
            status: TokenValidationStatus::Validated(
                Projects::wildcard_project(),
                env_status.clone(),
            ),
            ..EdgeToken::default()
        };
        let other_token = EdgeToken {
            status: TokenValidationStatus::Validated(
                Projects::new(vec!["A".into(), "B".into()]),
                env_status.clone(),
            ),
            ..EdgeToken::default()
        };

        let is_covered =
            self_token.same_environment_and_broader_or_equal_project_access(&other_token);
        assert!(is_covered);
    }

    #[test]
    fn test_multi_project_tokens_cover_each_other() {
        let env_status = EnvironmentStatus::Known("development".into());
        let self_token = EdgeToken {
            status: TokenValidationStatus::Validated(
                Projects::new(vec!["A".into(), "B".into()]),
                env_status.clone(),
            ),
            ..EdgeToken::default()
        };

        let fe_token = EdgeToken {
            status: TokenValidationStatus::Validated(Projects::single("A"), env_status.clone()),
            token_type: Some(TokenType::Frontend),
            ..EdgeToken::default()
        };

        let is_covered = self_token.same_environment_and_broader_or_equal_project_access(&fe_token);
        assert!(is_covered);
    }

    #[test]
    fn test_multi_project_tokens_do_not_cover_each_other_when_they_do_not_overlap() {
        let self_token = EdgeToken::default()
            .validated_with_projects(vec!["A".into(), "B".into()])
            .validated_with_environment("development");

        let mut fe_token = EdgeToken::default()
            .validated_with_projects(vec!["A".into(), "C".into()])
            .validated_with_environment("development");
        fe_token.token_type = Some(TokenType::Frontend);

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
}
