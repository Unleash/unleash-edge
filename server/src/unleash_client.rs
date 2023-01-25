use std::time::Duration;

use actix_web::{
    http::{
        header::{ContentType, EntityTag, IfNoneMatch},
        StatusCode,
    },
    Either,
};
use awc::{Client, ClientRequest};
use ulid::Ulid;
use unleash_types::{
    client_features::ClientFeatures,
    client_metrics::{ClientApplication, ClientMetrics},
};
use url::Url;

use crate::{error::EdgeError, types::ClientFeaturesRequest};

const UNLEASH_APPNAME_HEADER: &str = "UNLEASH-APPNAME";
const UNLEASH_INSTANCE_ID_HEADER: &str = "UNLEASH-INSTANCEID";
const UNLEASH_CLIENT_SPEC_HEADER: &str = "Unleash-Client-Spec";
const USER_AGENT_HEADER: &str = "User-Agent";

#[derive(Clone)]
pub struct UnleashClient {
    server_url: url::Url,
    backing_client: Client,
}

pub fn new_awc_client(instance_id: String) -> Client {
    awc::Client::builder()
        .add_default_header((UNLEASH_APPNAME_HEADER, "unleash-edge"))
        .add_default_header((UNLEASH_INSTANCE_ID_HEADER, instance_id))
        .add_default_header(
            (UNLEASH_CLIENT_SPEC_HEADER, "4.2.2"), // yggdrasil::CLIENT_SPEC_VERSION).into(),
        )
        .add_default_header((USER_AGENT_HEADER, "unleash_edge"))
        .add_default_header(ContentType::json())
        .timeout(Duration::from_secs(5))
        .finish()
}

impl UnleashClient {
    pub fn from_url(server_url: Url) -> Self {
        Self {
            server_url,
            backing_client: new_awc_client(Ulid::new().to_string()),
        }
    }
    pub fn new(server_url: String, instance_id_opt: Option<String>) -> Result<Self, EdgeError> {
        let url = Url::parse(server_url.as_str())
            .map_err(|urlparse| EdgeError::InvalidServerUrl(urlparse.to_string()))?;
        let instance_id = instance_id_opt.unwrap_or(Ulid::new().to_string());
        Ok(Self {
            server_url: url,
            backing_client: new_awc_client(instance_id.clone()),
        })
    }

    fn client_api_url(&self) -> Url {
        self.server_url
            .join("/api/client")
            .expect("Invalid client api")
    }
    fn client_features_url(&self) -> Url {
        self.client_api_url()
            .join("/features")
            .expect("Invalid URL for client_features")
    }

    fn register_app_url(&self) -> Url {
        self.client_api_url()
            .join("/register")
            .expect("Invalid URL for register app")
    }
    fn client_metrics_url(&self) -> Url {
        self.client_api_url()
            .join("/metrics")
            .expect("Invalid URL for client metrics")
    }
    fn edge_url(&self) -> Url {
        self.server_url.join("/edge").expect("Invalid URL for Edge")
    }
    fn edge_bulk_metrics_url(&self) -> Url {
        self.edge_url()
            .join("/metrics/bulk")
            .expect("Invalid URL for edge bulk metrics")
    }

    async fn register_app(&self, app: ClientApplication) -> Result<(), EdgeError> {
        todo!()
    }
    async fn post_metrics(&self, metrics: ClientMetrics) -> Result<(), EdgeError> {
        todo!()
    }

    fn awc_client_features_req(&self, req: ClientFeaturesRequest) -> ClientRequest {
        self.backing_client
            .get(self.client_features_url().to_string())
            .insert_header(IfNoneMatch::Items(vec![req.etag]))
            .insert_header(("Authorization", req.api_key))
    }

    async fn get_client_features(
        &self,
        request: ClientFeaturesRequest,
    ) -> Result<Either<(), ClientFeatures>, EdgeError> {
        let mut result = self
            .awc_client_features_req(request)
            .send()
            .await
            .map_err(|_| EdgeError::ClientFeaturesFetchError)?;
        if result.status() == StatusCode::NOT_MODIFIED {
            Ok(Either::Left(()))
        } else {
            result
                .json::<ClientFeatures>()
                .await
                .map(Either::Right)
                .map_err(|payload_error| EdgeError::ClientFeaturesParseError(payload_error))
        }
    }
}

#[cfg(test)]
mod tests {
    use test_case::test_case;

    use super::UnleashClient;

    #[test_case(
        "https://app.unleash-hosted.com/demo/",
        "https://app.unleash-hosted.com/demo/api/client/features" ; "With trailing slash in base url"
    )]
    #[test_case(
        "https://app.unleash-hosted.com/demo",
        "https://app.unleash-hosted.com/demo/api/client/features" ; "With no trailing slash in base url"
    )]
    fn can_build_client_features_url_from_server_url(base_url: &str, expected_url: &str) {
        let client = UnleashClient::new(base_url.into(), None).unwrap();
        assert_eq!(
            client.client_features_url().to_string(),
            expected_url.to_string()
        );
    }
}
