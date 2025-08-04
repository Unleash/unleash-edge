use dashmap::DashMap;
use std::sync::{Arc};
use tokio::sync::RwLock;
use ulid::Ulid;
use unleash_edge_auth::token_validator::TokenValidator;
use unleash_edge_cli::{AuthHeaders, EdgeMode};
use unleash_edge_feature_cache::FeatureCache;
use unleash_edge_feature_refresh::FeatureRefresher;
use unleash_edge_types::metrics::MetricsCache;
use unleash_edge_types::{EngineCache, TokenCache};
use unleash_types::client_metrics::ConnectVia;
use unleash_edge_types::metrics::instance_data::EdgeInstanceData;

#[derive(Clone)]
pub struct AppState {
    pub token_cache: Arc<TokenCache>,
    pub features_cache: Arc<FeatureCache>,
    pub engine_cache: Arc<EngineCache>,
    pub feature_refresher: Arc<Option<FeatureRefresher>>,
    pub token_validator: Arc<Option<TokenValidator>>,
    pub metrics_cache: Arc<MetricsCache>,
    pub offline_mode: bool,
    pub auth_headers: AuthHeaders,
    pub edge_mode: EdgeMode,
    pub connect_via: ConnectVia,
    pub edge_instance_data: EdgeInstanceData,
    pub connected_instances: Arc<RwLock<Vec<EdgeInstanceData>>>
}

impl AppState {
    pub fn builder() -> AppStateBuilder {
        AppStateBuilder::new("unleash-edge", Ulid::new())
    }

    pub fn builder_with_app_name_and_ulid(app_name: &str, ulid: Ulid) -> AppStateBuilder {
        AppStateBuilder::new(app_name, ulid)
    }
}

pub struct AppStateBuilder {
    token_cache: Arc<TokenCache>,
    features_cache: Arc<FeatureCache>,
    engine_cache: Arc<EngineCache>,
    feature_refresher: Arc<Option<FeatureRefresher>>,
    token_validator: Arc<Option<TokenValidator>>,
    metrics_cache: Arc<MetricsCache>,
    offline_mode: bool,
    auth_headers: AuthHeaders,
    edge_mode: EdgeMode,
    connect_via: ConnectVia,
    edge_instance_data: EdgeInstanceData,
    connected_instances: Arc<RwLock<Vec<EdgeInstanceData>>>
}

impl AppStateBuilder {
    pub fn new(app_name: &str, instance_id: Ulid) -> Self {
        Self {
            token_cache: Arc::new(DashMap::new()),
            features_cache: Arc::new(FeatureCache::new(DashMap::default())),
            engine_cache: Arc::new(DashMap::new()),
            feature_refresher: Arc::new(None),
            token_validator: Arc::new(None),
            metrics_cache: Arc::new(MetricsCache::default()),
            offline_mode: false,
            auth_headers: AuthHeaders::default(),
            edge_mode: EdgeMode::default(),
            connect_via: ConnectVia {
                app_name: app_name.to_string(),
                instance_id: instance_id.to_string(),
            },
            edge_instance_data: EdgeInstanceData::new(app_name, &instance_id),
            connected_instances: Arc::new(RwLock::new(Vec::new())),

        }
    }

    pub fn with_token_validator(mut self, token_validator: Arc<Option<TokenValidator>>) -> Self {
        self.token_validator = token_validator.clone();
        self
    }

    pub fn with_metrics_cache(mut self, metrics_cache: Arc<MetricsCache>) -> Self {
        self.metrics_cache = metrics_cache;
        self
    }

    pub fn with_offline_mode(mut self, offline_mode: bool) -> Self {
        self.offline_mode = offline_mode;
        self
    }

    pub fn with_features_cache(mut self, features_cache: Arc<FeatureCache>) -> Self {
        self.features_cache = features_cache;
        self
    }

    pub fn with_feature_refresher(
        mut self,
        feature_refresher: Arc<Option<FeatureRefresher>>,
    ) -> Self {
        self.feature_refresher = feature_refresher;
        self
    }

    pub fn with_token_cache(mut self, token_cache: Arc<TokenCache>) -> Self {
        self.token_cache = token_cache;
        self
    }

    pub fn with_edge_mode(mut self, edge_mode: EdgeMode) -> Self {
        self.edge_mode = edge_mode;
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

    pub fn build(self) -> AppState {
        AppState {
            token_cache: self.token_cache,
            features_cache: self.features_cache,
            engine_cache: self.engine_cache,
            feature_refresher: self.feature_refresher,
            token_validator: self.token_validator,
            metrics_cache: self.metrics_cache,
            offline_mode: self.offline_mode,
            auth_headers: self.auth_headers,
            edge_mode: self.edge_mode,
            connect_via: self.connect_via,
            edge_instance_data: self.edge_instance_data,
            connected_instances: self.connected_instances,
        }
    }
}

pub mod edge_token_extractor;
