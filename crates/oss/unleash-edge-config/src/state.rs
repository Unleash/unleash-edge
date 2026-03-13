use crate::auth::AuthHeaderConfig;
use crate::httpclient::ClientMetaInformation;
use crate::metrics::PrometheusConfig;
use crate::otel::TracingMode;
pub use crate::persistence::PersistenceConfig;
use crate::state::RemoteWriteConfig::Prometheus;
use ipnet::IpNet;
use std::sync::Arc;
use tokio::sync::RwLock;
use ulid::Ulid;
use unleash_edge_cli::EdgeArgs;
use unleash_edge_types::metrics::instance_data::{EdgeInstanceData, Hosting};
use unleash_edge_types::tokens::EdgeToken;
use unleash_edge_types::urls::UnleashUrls;

#[derive(Debug, Clone)]
pub enum RemoteWriteConfig {
    Prometheus(PrometheusConfig),
    NoOp,
}

impl From<&EdgeArgs> for RemoteWriteConfig {
    fn from(value: &EdgeArgs) -> Self {
        if let Some(remote_url) = value.prometheus_remote_write_url.as_ref() {
            Prometheus(PrometheusConfig {
                remote_write_url: remote_url.clone(),
                push_interval: value.prometheus_push_interval,
                username: value.prometheus_username.clone(),
                password: value.prometheus_password.clone(),
            })
        } else {
            RemoteWriteConfig::NoOp
        }
    }
}

pub type ClientHeader = (String, String);
pub type PreTrustedToken = (String, EdgeToken);
#[derive(Debug, Clone)]
pub struct EdgeStateConfig {
    pub app_id: Ulid,
    pub auth_header_config: AuthHeaderConfig,
    pub base_path: String,
    pub client_id: String,
    pub client_meta_information: ClientMetaInformation,
    pub custom_client_headers: Vec<ClientHeader>,
    pub delta: bool,
    pub hosting_type: Hosting,
    pub http_allow_list: Vec<IpNet>,
    pub http_client: reqwest::Client,
    pub http_deny_list: Vec<IpNet>,
    pub instances_observed_for_app_context: Arc<RwLock<Vec<EdgeInstanceData>>>,
    pub persistence: PersistenceConfig,
    pub remote_write_config: RemoteWriteConfig,
    pub streaming: bool,
    pub tokens: Vec<EdgeToken>,
    pub tracing_mode: TracingMode,
    pub unleash_urls: UnleashUrls,
    pub pretrusted_tokens: Vec<PreTrustedToken>,
    pub features_refresh_interval: chrono::Duration,
    pub metrics_interval_seconds: chrono::Duration,
    pub token_revalidation_interval_seconds: chrono::Duration,
}
