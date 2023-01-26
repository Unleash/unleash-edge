use url::Url;

use crate::error::EdgeError;

#[derive(Clone, Debug)]
pub struct UnleashUrls {
    pub base_url: Url,
    pub api_url: Url,
    pub client_api_url: Url,
    pub client_features_url: Url,
    pub client_register_app_url: Url,
    pub client_metrics_url: Url,
    pub edge_api_url: Url,
    pub edge_validate_url: Url,
}

impl UnleashUrls {
    pub fn from_str(base_url: &str) -> Result<Self, EdgeError> {
        Url::parse(base_url)
            .map_err(|e| EdgeError::InvalidServerUrl(base_url.into()))
            .map(|base| UnleashUrls::from_base_url(base))
    }
    pub fn from_base_url(base_url: Url) -> Self {
        let api_url = base_url.join("/api").expect("Could not create /api url");
        let client_api_url = api_url
            .join("/client")
            .expect("Could not create /api/client url");
        let client_features_url = client_api_url
            .join("/features")
            .expect("Could not create /api/client/features url");
        let client_register_app_url = client_api_url
            .join("/register")
            .expect("Could not create /api/client/register url");
        let client_metrics_url = client_api_url
            .join("/metrics")
            .expect("Could not create /api/client/metrics url");
        let edge_api_url = base_url.join("/edge").expect("Could not create /edge url");
        let edge_validate_url = edge_api_url
            .join("/validate")
            .expect("Could not create /edge/validate url");
        UnleashUrls {
            base_url,
            api_url,
            client_api_url,
            client_features_url,
            client_register_app_url,
            client_metrics_url,
            edge_api_url,
            edge_validate_url,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_case::test_case;

    #[test_case("https://app.unleash-hosted.com/demo", "https://app.unleash-hosted.com/demo" ; "No trailing slash, https protocol")]
    #[test_case("https://app.unleash-hosted.com/demo/", "https://app.unleash-hosted.com/demo" ; "One trailing slash, https protocol")]
    #[test_case("http://app.unleash-hosted.com/demo/", "http://app.unleash-hosted.com/demo" ; "One trailing slash, http protocol")]
    #[test_case("https://app.unleash-hosted.com/demo////", "http://app.unleash-hosted.com/demo" ; "Multiple trailing slashes")]
    pub fn can_handle_base_urls(base_url: &str, expected_base: &str) {
        let urls = UnleashUrls::from_str(base_url).unwrap();
        assert_eq!(urls.base_url.to_string(), expected_base);
    }
}
