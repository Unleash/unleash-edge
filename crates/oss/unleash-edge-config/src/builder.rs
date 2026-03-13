use crate::auth::AuthHeaderConfig;
use crate::httpclient::ClientMetaInformation;
use crate::persistence::PersistenceConfig;
use crate::state::{EdgeStateConfig, PreTrustedToken};
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;
use unleash_edge_types::metrics::instance_data::EdgeInstanceData;
use unleash_edge_types::tokens::EdgeToken;
use unleash_edge_types::urls::UnleashUrls;

pub struct EdgeBuilderOpts {
    pub streaming: bool,
    pub delta: bool,
    pub unleash_urls: UnleashUrls,
    pub client_meta_information: ClientMetaInformation,
    pub edge_instance_data: Arc<EdgeInstanceData>,
    pub auth_header_config: AuthHeaderConfig,
    pub http_client: reqwest::Client,
    pub tokens: Vec<EdgeToken>,
    pub custom_client_headers: Vec<(String, String)>,
    pub persistence_config: PersistenceConfig,
    pub deferred_validation: Option<UnboundedSender<String>>,
    pub pretrusted_tokens: Vec<PreTrustedToken>,
    pub features_refresh_interval: chrono::Duration,
}

impl EdgeBuilderOpts {
    pub fn from_edge_config_instance_data_and_deferred_validation(
        config: &EdgeStateConfig,
        instance_data: Arc<EdgeInstanceData>,
        deferred_validation: Option<UnboundedSender<String>>,
    ) -> Self {
        Self {
            streaming: config.streaming,
            delta: config.delta,
            unleash_urls: config.unleash_urls.clone(),
            client_meta_information: config.client_meta_information.clone(),
            edge_instance_data: instance_data,
            auth_header_config: config.auth_header_config.clone(),
            http_client: config.http_client.clone(),
            tokens: config.tokens.clone(),
            custom_client_headers: config.custom_client_headers.clone(),
            persistence_config: config.persistence.clone(),
            deferred_validation,
            pretrusted_tokens: config.pretrusted_tokens.clone(),
            features_refresh_interval: config.features_refresh_interval,
        }
    }
}
