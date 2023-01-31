use std::str::FromStr;
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

use crate::types::EdgeResult;
use crate::urls::UnleashUrls;
use crate::{error::EdgeError, types::ClientFeaturesRequest};

const UNLEASH_APPNAME_HEADER: &str = "UNLEASH-APPNAME";
const UNLEASH_INSTANCE_ID_HEADER: &str = "UNLEASH-INSTANCEID";
const UNLEASH_CLIENT_SPEC_HEADER: &str = "Unleash-Client-Spec";
const USER_AGENT_HEADER: &str = "User-Agent";

#[derive(Clone)]
pub struct UnleashClient {
    pub urls: UnleashUrls,
    backing_client: Client,
}

pub fn new_awc_client(instance_id: String) -> Client {
    Client::builder()
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
            urls: UnleashUrls::from_base_url(server_url),
            backing_client: new_awc_client(Ulid::new().to_string()),
        }
    }
    pub fn new(server_url: &str, instance_id_opt: Option<String>) -> Result<Self, EdgeError> {
        let instance_id = instance_id_opt.unwrap_or(Ulid::new().to_string());
        Ok(Self {
            urls: UnleashUrls::from_str(server_url)?,
            backing_client: new_awc_client(instance_id),
        })
    }

    async fn register_app(&self, app: ClientApplication) -> Result<(), EdgeError> {
        todo!()
    }
    async fn post_metrics(&self, metrics: ClientMetrics) -> Result<(), EdgeError> {
        todo!()
    }

    fn awc_client_features_req(&self, req: ClientFeaturesRequest) -> ClientRequest {
        self.backing_client
            .get(self.urls.client_features_url.to_string())
            .insert_header(IfNoneMatch::Items(vec![req.etag]))
            .insert_header(("Authorization", req.api_key))
    }

    pub async fn get_client_features(
        &self,
        request: ClientFeaturesRequest,
    ) -> EdgeResult<Either<(), ClientFeatures>> {
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
