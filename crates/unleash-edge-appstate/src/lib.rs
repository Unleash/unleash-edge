use dashmap::DashMap;
use ipnet::IpNet;
use std::sync::Arc;
use tokio::sync::RwLock;
use ulid::Ulid;
use unleash_edge_auth::token_validator::TokenValidator;
use unleash_edge_cli::AuthHeaders;
use unleash_edge_delta::cache_manager::DeltaCacheManager;
use unleash_edge_feature_cache::FeatureCache;
use unleash_edge_feature_refresh::HydratorType;
use unleash_edge_types::metrics::MetricsCache;
use unleash_edge_types::metrics::instance_data::{EdgeInstanceData, Hosting};
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
    auth_headers: AuthHeaders,
    pub connect_via: ConnectVia,
    pub edge_instance_data: Arc<EdgeInstanceData>,
    pub connected_instances: Arc<RwLock<Vec<EdgeInstanceData>>>,
    pub deny_list: Vec<IpNet>,
    pub allow_list: Vec<IpNet>,
}

impl AppState {
    pub fn builder() -> AppStateBuilder {
        AppStateBuilder::new("unleash-edge", Ulid::new())
    }
}

pub struct AppStateBuilder {
    token_cache: Arc<TokenCache>,
    features_cache: Arc<FeatureCache>,
    engine_cache: Arc<EngineCache>,
    hydrator: Option<HydratorType>,
    token_validator: Arc<Option<TokenValidator>>,
    metrics_cache: Arc<MetricsCache>,
    delta_cache_manager: Option<Arc<DeltaCacheManager>>,
    auth_headers: AuthHeaders,
    connect_via: ConnectVia,
    edge_instance_data: Arc<EdgeInstanceData>,
    connected_instances: Arc<RwLock<Vec<EdgeInstanceData>>>,
    deny_list: Vec<IpNet>,
    allow_list: Vec<IpNet>,
}

impl AppStateBuilder {
    pub fn new(app_name: &str, instance_id: Ulid) -> Self {
        let hosting = Hosting::from_env();
        Self {
            token_cache: Arc::new(DashMap::new()),
            features_cache: Arc::new(FeatureCache::new(DashMap::default())),
            engine_cache: Arc::new(DashMap::new()),
            hydrator: None,
            token_validator: Arc::new(None),
            metrics_cache: Arc::new(MetricsCache::default()),
            auth_headers: AuthHeaders::default(),
            connect_via: ConnectVia {
                app_name: app_name.to_string(),
                instance_id: instance_id.to_string(),
            },
            edge_instance_data: Arc::new(EdgeInstanceData::new(
                app_name,
                &instance_id,
                Some(hosting),
            )),
            connected_instances: Arc::new(RwLock::new(Vec::new())),
            delta_cache_manager: None,
            deny_list: vec![],
            allow_list: vec![],
        }
    }

    pub fn with_connected_instances(
        mut self,
        connected_instances: Arc<RwLock<Vec<EdgeInstanceData>>>,
    ) -> Self {
        self.connected_instances = connected_instances;
        self
    }
    pub fn with_token_validator(mut self, token_validator: Arc<Option<TokenValidator>>) -> Self {
        self.token_validator = token_validator.clone();
        self
    }

    pub fn with_metrics_cache(mut self, metrics_cache: Arc<MetricsCache>) -> Self {
        self.metrics_cache = metrics_cache;
        self
    }

    pub fn with_features_cache(mut self, features_cache: Arc<FeatureCache>) -> Self {
        self.features_cache = features_cache;
        self
    }

    pub fn with_token_cache(mut self, token_cache: Arc<TokenCache>) -> Self {
        self.token_cache = token_cache;
        self
    }

    pub fn with_auth_headers(mut self, auth_headers: AuthHeaders) -> Self {
        self.auth_headers = auth_headers;
        self
    }

    pub fn with_engine_cache(mut self, engine_cache: Arc<EngineCache>) -> Self {
        self.engine_cache = engine_cache;
        self
    }

    pub fn with_deny_list(mut self, deny_list: Vec<IpNet>) -> Self {
        self.deny_list = deny_list;
        self
    }

    pub fn with_allow_list(mut self, allow_list: Vec<IpNet>) -> Self {
        self.allow_list = allow_list;
        self
    }

    pub fn with_edge_instance_data(mut self, edge_instance_data: Arc<EdgeInstanceData>) -> Self {
        self.edge_instance_data = edge_instance_data;
        self
    }

    pub fn with_delta_cache_manager(mut self, delta_cache_manager: Arc<DeltaCacheManager>) -> Self {
        self.delta_cache_manager = Some(delta_cache_manager);
        self
    }

    pub fn with_hydrator(mut self, hydrator: HydratorType) -> Self {
        self.hydrator = Some(hydrator);
        self
    }

    pub fn with_connect_via(mut self, connect_via: ConnectVia) -> Self {
        self.connect_via = connect_via;
        self
    }

    pub fn build(self) -> AppState {
        AppState {
            token_cache: self.token_cache,
            features_cache: self.features_cache,
            engine_cache: self.engine_cache,
            hydrator: self.hydrator,
            token_validator: self.token_validator,
            metrics_cache: self.metrics_cache,
            auth_headers: self.auth_headers,
            connect_via: self.connect_via,
            edge_instance_data: self.edge_instance_data,
            connected_instances: self.connected_instances,
            deny_list: self.deny_list,
            allow_list: self.allow_list,
            delta_cache_manager: self.delta_cache_manager,
        }
    }
}

pub mod edge_token_extractor;
pub mod token_cache_observer;
