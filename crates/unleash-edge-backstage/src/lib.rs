use axum::extract::{FromRef, State};
use axum::routing::get;
use axum::{Json, Router};
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use unleash_edge_appstate::AppState;
use unleash_edge_auth::token_validator::TokenValidator;
use unleash_edge_cli::InternalBackstageArgs;
use unleash_edge_feature_cache::FeatureCache;
use unleash_edge_feature_refresh::HydratorType;
use unleash_edge_types::errors::EdgeError;
use unleash_edge_types::metrics::MetricsCache;
use unleash_edge_types::metrics::instance_data::EdgeInstanceData;
use unleash_edge_types::tokens::{EdgeToken, anonymize_token};
use unleash_edge_types::{
    BuildInfo, ClientMetric, EdgeJsonResult, MetricsInfo, Status, TokenCache, TokenInfo,
    TokenRefresh, TokenValidationStatus,
};
use unleash_types::client_features::ClientFeatures;
use unleash_types::client_metrics::ClientApplication;

#[derive(Debug, Serialize, Deserialize)]
pub struct EdgeStatus {
    pub status: Status,
}

impl EdgeStatus {
    pub fn ok() -> Self {
        EdgeStatus { status: Status::Ok }
    }
    pub fn not_ready() -> Self {
        EdgeStatus {
            status: Status::NotReady,
        }
    }

    pub fn ready() -> Self {
        EdgeStatus {
            status: Status::Ready,
        }
    }
}

pub async fn health() -> EdgeJsonResult<EdgeStatus> {
    Ok(Json(EdgeStatus::ok()))
}

#[derive(Clone)]
pub struct TokenCheckState {
    pub token_cache: Arc<TokenCache>,
    pub token_validator: Arc<Option<TokenValidator>>,
    pub hydrator: Option<HydratorType>,
}

pub struct ReadyCheckState {
    pub token_cache: Arc<TokenCache>,
    pub features_cache: Arc<FeatureCache>,
}

pub struct MetricsCheckState {
    pub metrics_cache: Arc<MetricsCache>,
}

pub struct FeatureCheckState {
    pub features_cache: Arc<FeatureCache>,
}

pub struct InstanceDataCheckState {
    pub edge_instance_data: Arc<EdgeInstanceData>,
    pub connected_instances: Arc<RwLock<Vec<EdgeInstanceData>>>,
}

impl FromRef<AppState> for TokenCheckState {
    fn from_ref(app_state: &AppState) -> Self {
        TokenCheckState {
            token_cache: app_state.token_cache.clone(),
            token_validator: app_state.token_validator.clone(),
            hydrator: app_state.hydrator.clone(),
        }
    }
}

impl FromRef<AppState> for ReadyCheckState {
    fn from_ref(app_state: &AppState) -> Self {
        ReadyCheckState {
            token_cache: app_state.token_cache.clone(),
            features_cache: app_state.features_cache.clone(),
        }
    }
}

impl FromRef<AppState> for MetricsCheckState {
    fn from_ref(app_state: &AppState) -> Self {
        MetricsCheckState {
            metrics_cache: app_state.metrics_cache.clone(),
        }
    }
}

impl FromRef<AppState> for FeatureCheckState {
    fn from_ref(app_state: &AppState) -> Self {
        FeatureCheckState {
            features_cache: app_state.features_cache.clone(),
        }
    }
}

impl FromRef<AppState> for InstanceDataCheckState {
    fn from_ref(app_state: &AppState) -> Self {
        InstanceDataCheckState {
            edge_instance_data: app_state.edge_instance_data.clone(),
            connected_instances: app_state.connected_instances.clone(),
        }
    }
}

pub async fn ready(app_state: State<ReadyCheckState>) -> EdgeJsonResult<EdgeStatus> {
    if !app_state.token_cache.is_empty() && app_state.features_cache.is_empty() {
        Err(EdgeError::NotReady)
    } else {
        Ok(Json(EdgeStatus::ready()))
    }
}

pub async fn info() -> EdgeJsonResult<BuildInfo> {
    Ok(Json(BuildInfo::default()))
}

pub async fn tokens(app_state: State<TokenCheckState>) -> EdgeJsonResult<TokenInfo> {
    if app_state.hydrator.is_some() && app_state.token_validator.is_some() {
        Ok(Json(get_token_info(app_state.0)))
    } else {
        Ok(Json(get_offline_token_info(app_state.token_cache.clone())))
    }
}

fn get_token_info(app_state: TokenCheckState) -> TokenInfo {
    let refreshes: Vec<TokenRefresh> = app_state
        .hydrator
        .map(|refresher| {
            refresher
                .tokens_to_refresh()
                .iter()
                .map(|e| e.value().clone())
                .map(|f| TokenRefresh {
                    token: anonymize_token(&f.token),
                    ..f.clone()
                })
                .collect()
        })
        .unwrap_or_default();

    let token_validation_status: Vec<EdgeToken> = (*app_state.token_validator)
        .clone()
        .unwrap()
        .token_cache
        .iter()
        .filter(|t| {
            t.status == TokenValidationStatus::Validated
                || t.status == TokenValidationStatus::Trusted
        })
        .map(|e| e.value().clone())
        .map(|t| anonymize_token(&t))
        .collect();
    let invalid_token_count = (*app_state.token_validator)
        .clone()
        .unwrap()
        .token_cache
        .iter()
        .filter(|t| {
            t.status == TokenValidationStatus::Invalid || t.status == TokenValidationStatus::Unknown
        })
        .count();
    TokenInfo {
        token_refreshes: refreshes,
        token_validation_status,
        invalid_token_count,
    }
}

fn get_offline_token_info(token_cache: Arc<TokenCache>) -> TokenInfo {
    let edge_tokens: Vec<EdgeToken> = token_cache
        .iter()
        .map(|e| e.value().clone())
        .map(|t| anonymize_token(&t))
        .collect();
    TokenInfo {
        token_refreshes: vec![],
        token_validation_status: edge_tokens,
        invalid_token_count: 0,
    }
}

pub async fn metrics_batch(app_state: State<MetricsCheckState>) -> EdgeJsonResult<MetricsInfo> {
    let applications: Vec<ClientApplication> = app_state
        .metrics_cache
        .applications
        .iter()
        .map(|e| e.value().clone())
        .collect_vec();
    let metrics = app_state
        .metrics_cache
        .metrics
        .iter()
        .map(|e| ClientMetric {
            key: e.key().clone(),
            bucket: e.value().clone(),
        })
        .collect_vec();
    Ok(Json(MetricsInfo {
        applications,
        metrics,
    }))
}

pub async fn features(
    app_state: State<FeatureCheckState>,
) -> EdgeJsonResult<HashMap<String, ClientFeatures>> {
    let features = app_state
        .features_cache
        .iter()
        .map(|e| (e.key().clone(), e.value().clone()))
        .collect();
    Ok(Json(features))
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DebugEdgeInstanceData {
    pub this_instance: EdgeInstanceData,
    pub connected_instances: Vec<EdgeInstanceData>,
}

pub async fn instance_data(
    app_state: State<InstanceDataCheckState>,
) -> EdgeJsonResult<DebugEdgeInstanceData> {
    Ok(Json(DebugEdgeInstanceData {
        this_instance: app_state.edge_instance_data.as_ref().clone(),
        connected_instances: app_state.connected_instances.read().await.clone(),
    }))
}

pub fn router(internal_backstage_args: InternalBackstageArgs) -> Router<AppState> {
    let mut router = Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready));
    if !internal_backstage_args.disable_features_endpoint {
        router = router.route("/features", get(features));
    }
    if !internal_backstage_args.disable_tokens_endpoint {
        router = router.route("/tokens", get(tokens));
    }
    if !internal_backstage_args.disable_metrics_batch_endpoint {
        router = router.route("/metricsbatch", get(metrics_batch));
    }
    if !internal_backstage_args.disable_instance_data_endpoint {
        router = router.route("/instancedata", get(instance_data));
    }
    router
}
