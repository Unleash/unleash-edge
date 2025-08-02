use crate::EdgeResult;
use crate::errors::EdgeError;
use reqwest::Url;
use std::str::FromStr;

#[derive(Clone, Debug)]
pub struct UnleashUrls {
    pub base_url: Url,
    pub api_url: Url,
    pub client_api_url: Url,
    pub client_features_url: Url,
    pub client_features_delta_url: Url,
    pub client_register_app_url: Url,
    pub client_metrics_url: Url,
    pub client_bulk_metrics_url: Url,
    pub edge_api_url: Url,
    pub edge_validate_url: Url,
    pub edge_metrics_url: Url,
    pub edge_instance_data_url: Url,
    pub new_api_token_url: Url,
    pub client_features_stream_url: Url,
}

impl FromStr for UnleashUrls {
    type Err = EdgeError;
    fn from_str(base_url: &str) -> EdgeResult<Self> {
        let mut base =
            Url::parse(base_url).map_err(|_e| EdgeError::InvalidServerUrl(base_url.into()))?;
        base.path_segments_mut()
            .expect("Could not get path")
            .pop_if_empty();
        Ok(UnleashUrls::from_base_url(base))
    }
}
impl Default for UnleashUrls {
    fn default() -> Self {
        UnleashUrls::from_str("http://localhost:4242")
            .expect("Our valid base url was suddenly not accepted")
    }
}

impl UnleashUrls {
    pub fn from_base_url(base_url: Url) -> Self {
        let mut api_url = base_url.clone();
        api_url.path_segments_mut().unwrap().push("api");

        let mut client_api_url = api_url.clone();
        client_api_url.path_segments_mut().unwrap().push("client");
        let mut client_features_url = client_api_url.clone();
        client_features_url
            .path_segments_mut()
            .unwrap()
            .push("features");
        let mut client_features_delta_url = client_api_url.clone();
        client_features_delta_url
            .path_segments_mut()
            .unwrap()
            .push("delta");
        let mut client_features_stream_url = client_api_url.clone();
        client_features_stream_url
            .path_segments_mut()
            .unwrap()
            .push("streaming");
        let mut client_register_app_url = client_api_url.clone();
        client_register_app_url
            .path_segments_mut()
            .unwrap()
            .push("register");
        let mut client_metrics_url = client_api_url.clone();
        client_metrics_url
            .path_segments_mut()
            .expect("Couldn't get client metrics url")
            .push("metrics");
        let mut edge_api_url = base_url.clone();
        edge_api_url
            .path_segments_mut()
            .expect("Could not create /edge url")
            .push("edge");
        let mut edge_validate_url = edge_api_url.clone();
        edge_validate_url
            .path_segments_mut()
            .expect("Could not create /edge/validate url")
            .push("validate");
        let mut edge_metrics_url = edge_api_url.clone();
        edge_metrics_url
            .path_segments_mut()
            .expect("Could not create /edge/metrics")
            .push("metrics");

        let mut new_api_token_url = base_url.clone();
        new_api_token_url
            .path_segments_mut()
            .expect("Could not create /api/admin/api-tokens")
            .push("api")
            .push("admin")
            .push("api-tokens");

        let mut client_bulk_metrics_url = client_metrics_url.clone();
        client_bulk_metrics_url
            .path_segments_mut()
            .expect("Could not create /api/client/metrics/bulk")
            .push("bulk");
        let mut edge_instance_data_url = client_metrics_url.clone();
        edge_instance_data_url
            .path_segments_mut()
            .expect("Could not create /api/client/metrics/instance-data")
            .push("edge");
        UnleashUrls {
            base_url,
            api_url,
            client_api_url,
            client_features_url,
            client_features_delta_url,
            client_register_app_url,
            client_bulk_metrics_url,
            client_metrics_url,
            edge_api_url,
            edge_validate_url,
            edge_metrics_url,
            new_api_token_url,
            client_features_stream_url,
            edge_instance_data_url,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_case::test_case;

    #[test_case("https://app.unleash-hosted.com/demo", "https://app.unleash-hosted.com/demo/api", "https://app.unleash-hosted.com/demo/api/client", "https://app.unleash-hosted.com/demo/api/client/features", "https://app.unleash-hosted.com/demo/api/client/delta" ; "No trailing slash, https protocol")]
    #[test_case("https://app.unleash-hosted.com/demo/", "https://app.unleash-hosted.com/demo/api", "https://app.unleash-hosted.com/demo/api/client", "https://app.unleash-hosted.com/demo/api/client/features", "https://app.unleash-hosted.com/demo/api/client/delta" ; "One trailing slash, https protocol")]
    #[test_case("http://app.unleash-hosted.com/demo/", "http://app.unleash-hosted.com/demo/api", "http://app.unleash-hosted.com/demo/api/client", "http://app.unleash-hosted.com/demo/api/client/features", "http://app.unleash-hosted.com/demo/api/client/delta" ; "One trailing slash, http protocol")]
    #[test_case("http://app.unleash-hosted.com/", "http://app.unleash-hosted.com/api", "http://app.unleash-hosted.com/api/client", "http://app.unleash-hosted.com/api/client/features", "http://app.unleash-hosted.com/api/client/delta" ; "One trailing slash, no subpath, http protocol")]
    pub fn can_handle_base_urls(
        base_url: &str,
        api_url: &str,
        client_url: &str,
        client_features_url: &str,
        client_features_delta_url: &str,
    ) {
        let urls = UnleashUrls::from_str(base_url).unwrap();
        assert_eq!(urls.api_url.to_string(), api_url);
        assert_eq!(urls.client_api_url.to_string(), client_url);
        assert_eq!(urls.client_features_url.to_string(), client_features_url);
        assert_eq!(
            urls.client_features_delta_url.to_string(),
            client_features_delta_url
        );
    }
}
