use ipnet::IpNet;
use std::sync::Arc;
use tokio::sync::RwLock;
use unleash_edge_auth::token_validator::TokenValidator;
use unleash_edge_cli::AuthHeaders;
use unleash_edge_delta::cache_manager::DeltaCacheManager;
use unleash_edge_feature_cache::FeatureCache;
use unleash_edge_feature_refresh::HydratorType;
use unleash_edge_types::enterprise::LicenseState;
use unleash_edge_types::metrics::MetricsCache;
use unleash_edge_types::metrics::instance_data::EdgeInstanceData;
use unleash_edge_types::{EngineCache, TokenCache};
use unleash_types::client_metrics::ConnectVia;

#[derive(Clone)]
pub struct AppState {
    pub token_cache: Arc<TokenCache>,
    pub features_cache: Arc<FeatureCache>,
    pub engine_cache: Arc<EngineCache>,
    pub hydrator: Option<HydratorType>,
    pub token_validator: Arc<Option<TokenValidator>>,
    pub metrics_cache: Arc<MetricsCache>,
    pub delta_cache_manager: Option<Arc<DeltaCacheManager>>,
    pub auth_headers: AuthHeaders,
    pub connect_via: ConnectVia,
    pub edge_instance_data: Arc<EdgeInstanceData>,
    pub connected_instances: Arc<RwLock<Vec<EdgeInstanceData>>>,
    pub deny_list: Vec<IpNet>,
    pub allow_list: Vec<IpNet>,
    pub license_state: Arc<RwLock<LicenseState>>,
}

pub mod edge_token_extractor;
pub mod token_cache_observer;
