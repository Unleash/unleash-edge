use crate::types::TokenRefresh;

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
