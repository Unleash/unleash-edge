use crate::types::EdgeToken;

#[allow(dead_code)] // until used
pub(crate) fn simplify(tokens: &[EdgeToken]) -> Vec<EdgeToken> {
    tokens
        .iter()
        .filter_map(|token| {
            tokens.iter().fold(Some(token.clone()), |acc, current| {
                acc.and_then(|lead| {
                    if current != &lead && current.subsumes(&lead) {
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
    use crate::{tokens::simplify, types::EdgeToken};

    fn test_token(env: Option<&str>, projects: Vec<&str>) -> EdgeToken {
        EdgeToken {
            environment: env.map(|env| env.into()),
            projects: projects.into_iter().map(|p| p.into()).collect(),
            ..EdgeToken::default()
        }
    }

    #[test]
    fn test_case_1_token_with_two_projects_subsumes_tokens_having_individually_each_token() {
        let tokens = vec![
            test_token(None, vec!["p1", "p2"]),
            test_token(None, vec!["p1"]),
            test_token(None, vec!["p1"]),
        ];

        let expected = vec![test_token(None, vec!["p1", "p2"])];

        assert_eq!(simplify(&tokens), expected);
    }

    #[test]
    fn test_case_2_when_two_environments_are_different_we_have_at_least_two_tokens() {
        let tokens = vec![
            test_token(Some("env1"), vec!["p1", "p2"]),
            test_token(Some("env1"), vec!["p1"]),
            test_token(None, vec!["p1"]),
        ];
        let expected = vec![
            test_token(Some("env1"), vec!["p1", "p2"]),
            test_token(None, vec!["p1"]),
        ];

        assert_eq!(simplify(&tokens), expected);
    }

    #[test]
    fn test_case_3_star_token_subsumes_all_tokens() {
        let tokens = vec![
            test_token(None, vec!["p1"]),
            test_token(None, vec!["*"]),
            test_token(None, vec!["p1", "p2"]),
        ];
        let expected = vec![test_token(None, vec!["*"])];

        assert_eq!(simplify(&tokens), expected);
    }

    #[test]
    fn test_case_4_when_a_project_is_shared_between_two_tokens_we_simplify_as_much_as_we_can() {
        let tokens = vec![
            test_token(None, vec!["p1", "p2"]),
            test_token(Some("env"), vec!["p1", "p2"]),
            test_token(None, vec!["p1"]),
            test_token(Some("env"), vec!["p2", "p3"]),
            test_token(None, vec!["*"]),
            test_token(Some("env"), vec!["p1"]),
            test_token(None, vec!["p3"]),
            test_token(Some("env"), vec!["p2"]),
        ];
        let expected = vec![
            test_token(Some("env"), vec!["p1", "p2"]),
            test_token(Some("env"), vec!["p2", "p3"]),
            test_token(None, vec!["*"]),
        ];

        assert_eq!(simplify(&tokens), expected);
    }
}
