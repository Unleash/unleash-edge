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
    use crate::{
        tokens::simplify,
        types::{EdgeToken, TokenType},
    };

    fn test_token(env: Option<&str>, projects: Vec<&str>) -> EdgeToken {
        EdgeToken {
            secret: "the-secret".into(),
            token_type: Some(TokenType::Client),
            environment: env.map(|env| env.into()),
            projects: projects.into_iter().map(|p| p.into()).collect(),
            expires_at: None,
            seen_at: None,
            alias: None,
        }
    }

    #[test]
    fn test_case_1() {
        let tokens = vec![
            test_token(None, vec!["p1", "p2"]),
            test_token(None, vec!["p1"]),
            test_token(None, vec!["p1"]),
        ];

        let expected = vec![test_token(None, vec!["p1", "p2"])];

        assert_eq!(simplify(&tokens), expected);
    }

    #[test]
    fn test_case_2() {
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
    fn test_case_3() {
        let tokens = vec![
            test_token(None, vec!["p1"]),
            test_token(None, vec!["*"]),
            test_token(None, vec!["p1", "p2"]),
        ];
        let expected = vec![test_token(None, vec!["*"])];

        assert_eq!(simplify(&tokens), expected);
    }

    #[test]
    fn test_case_4() {
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
