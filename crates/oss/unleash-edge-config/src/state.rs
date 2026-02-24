use crate::auth::AuthHeaderConfig;
use crate::httpclient::ClientMetaInformation;
use crate::logging::LogFormat;
use crate::metrics::PrometheusConfig;
use crate::otel::TracingMode;
use crate::redis::RedisMode;
use ipnet::IpNet;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use ulid::Ulid;
use unleash_edge_types::metrics::instance_data::{EdgeInstanceData, Hosting};
use unleash_edge_types::tokens::EdgeToken;
use unleash_edge_types::urls::UnleashUrls;

#[derive(Debug, Clone)]
pub struct S3Opts {
    pub bucket_name: String,
}

#[derive(Debug, Clone)]
pub struct RedisOpts {
    pub redis_password: Option<String>,
    pub redis_mode: RedisMode,
    pub redis_url: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct FileOpts {}

#[derive(Debug, Clone, Default)]
pub enum PersistenceConfig {
    S3(S3Opts),
    Redis(RedisOpts),
    File(FileOpts),
    #[default]
    None,
}

#[derive(Debug, Clone)]
pub enum RemoteWriteConfig {
    Prometheus(PrometheusConfig),
    None,
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
    pub log_format: LogFormat,
    pub persistence: PersistenceConfig,
    pub remote_write_config: RemoteWriteConfig,
    pub streaming: bool,
    pub tokens: Vec<EdgeToken>,
    pub tracing_mode: TracingMode,
    pub unleash_urls: UnleashUrls,
    pub pretrusted_tokens: Vec<PreTrustedToken>,
    pub features_refresh_interval: Duration,
}
