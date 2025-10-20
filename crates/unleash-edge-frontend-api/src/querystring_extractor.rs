use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use serde::de::DeserializeOwned;
use serde_qs::Config;
use unleash_edge_types::errors::EdgeError;

/// Wrapper type like QsQuery, but with custom config.
pub struct QsQueryCfg<T>(pub T);

impl<S, T> FromRequestParts<S> for QsQueryCfg<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = EdgeError;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        let query_str = parts.uri.query().unwrap_or("");

        let cfg = Config::new(5, false);

        cfg.deserialize_str(query_str)
            .map(QsQueryCfg)
            .map_err(|_| EdgeError::ContextParseError)
    }
}

#[cfg(test)]
mod tests {
    use axum::http::Uri;
    use serde_qs::Config;
    use unleash_types::client_features::Context;

    #[test]
    pub fn query_parsing_works() {
        let uri =
            Uri::from_static("http://localhost/frontend?properties[companyId]=bricks&another=true");
        let cfg = Config::new(5, false);
        let q = cfg.deserialize_str::<Context>(uri.query().unwrap());
        assert!(q.is_ok());
        let q = q.unwrap();
        assert!(q.properties.clone().is_some());
        let props = q.properties.clone().unwrap();
        assert!(!props.is_empty());
        assert_eq!(props.get("companyId"), Some(&"bricks".to_string()));
    }
}

