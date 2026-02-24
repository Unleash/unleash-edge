use unleash_edge_cli::AuthHeaders;

#[derive(Debug, Clone)]
pub struct AuthHeaderConfig {
    pub edge_auth_header: String,
    pub upstream_auth_header: String,
}

impl Default for AuthHeaderConfig {
    fn default() -> Self {
        Self {
            edge_auth_header: AUTHORIZATION.into(),
            upstream_auth_header: AUTHORIZATION.into(),
        }
    }
}

const AUTHORIZATION: &str = "Authorization";

impl From<&AuthHeaders> for AuthHeaderConfig {
    fn from(headers: &AuthHeaders) -> Self {
        Self {
            edge_auth_header: headers
                .edge_auth_header
                .clone()
                .unwrap_or(AUTHORIZATION.into()),
            upstream_auth_header: headers
                .upstream_auth_header
                .clone()
                .unwrap_or(AUTHORIZATION.into()),
        }
    }
}
