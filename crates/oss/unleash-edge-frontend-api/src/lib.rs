use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::{Json, Router};
use itertools::Itertools;
use std::collections::HashMap;
use std::net::IpAddr;
use tracing::instrument;
use unleash_edge_appstate::AppState;
use unleash_edge_types::EdgeJsonResult;
use unleash_edge_types::errors::EdgeError;
use unleash_edge_types::tokens::{EdgeToken, cache_key};
use unleash_types::client_features::Context;
use unleash_types::frontend::{EvaluatedToggle, EvaluatedVariant, FrontendResult};
use unleash_yggdrasil::ResolvedToggle;

use crate::frontend::FrontendState;

pub struct UnleashSdkHeader(pub Option<String>);
impl<S> FromRequestParts<S> for UnleashSdkHeader
where
    S: Send + Sync,
{
    type Rejection = EdgeError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let ver = parts
            .headers
            .get("unleash-sdk")
            .and_then(|val| val.to_str().ok())
            .map(str::to_owned);
        Ok(UnleashSdkHeader(ver))
    }
}
pub mod frontend;
pub mod querystring_extractor;

pub fn router(disable_all_endpoints: bool) -> Router<AppState> {
    Router::new().merge(frontend::frontend_router_for(disable_all_endpoints))
}

#[instrument(skip(app_state, edge_token, context, client_ip))]
pub fn enabled_features(
    app_state: FrontendState,
    edge_token: EdgeToken,
    context: &Context,
    client_ip: IpAddr,
) -> EdgeJsonResult<FrontendResult> {
    let context_with_ip = if context.remote_address.is_none() {
        &Context {
            remote_address: Some(client_ip.to_string()),
            ..context.clone()
        }
    } else {
        context
    };
    let token = app_state
        .token_cache
        .get(&edge_token.token)
        .map(|e| e.value().clone())
        .unwrap_or_else(|| edge_token.clone());
    let key = cache_key(&token);
    let engine = app_state.engine_cache.get(&key).ok_or_else(|| {
        EdgeError::Forbidden("The token used does not have access to this edge".into())
    })?;
    let feature_results = engine.resolve_all(context_with_ip, &None).ok_or_else(|| {
        EdgeError::Forbidden("The token used does not have access to this edge".into())
    })?;
    Ok(Json(frontend_from_yggdrasil(
        feature_results,
        false,
        &token,
    )))
}

#[instrument(skip(app_state, edge_token, context, client_ip))]
pub fn all_features(
    app_state: FrontendState,
    edge_token: EdgeToken,
    context: &Context,
    client_ip: IpAddr,
) -> EdgeJsonResult<FrontendResult> {
    let context_with_ip = if context.remote_address.is_none() {
        &Context {
            remote_address: Some(client_ip.to_string()),
            ..context.clone()
        }
    } else {
        context
    };
    let token = app_state
        .token_cache
        .get(&edge_token.token)
        .map(|e| e.value().clone())
        .unwrap_or_else(|| edge_token.clone());
    let key = cache_key(&token);
    let engine = app_state.engine_cache.get(&key).ok_or_else(|| {
        EdgeError::Forbidden("The token used does not have access to this edge".into())
    })?;
    let feature_results = engine.resolve_all(context_with_ip, &None).ok_or_else(|| {
        EdgeError::Forbidden("The token used does not have access to this edge".into())
    })?;
    Ok(Json(frontend_from_yggdrasil(feature_results, true, &token)))
}

#[instrument(skip(res, include_all, edge_token))]
fn frontend_from_yggdrasil(
    res: HashMap<String, ResolvedToggle>,
    include_all: bool,
    edge_token: &EdgeToken,
) -> FrontendResult {
    let toggles: Vec<EvaluatedToggle> = res
        .iter()
        .filter(|(_, resolved)| include_all || resolved.enabled)
        .filter(|(_, resolved)| {
            edge_token.projects.is_empty()
                || edge_token.projects.contains(&"*".to_string())
                || edge_token.projects.contains(&resolved.project)
        })
        .map(|(name, resolved)| EvaluatedToggle {
            name: name.into(),
            enabled: resolved.enabled,
            variant: EvaluatedVariant {
                name: resolved.variant.name.clone(),
                enabled: resolved.variant.enabled,
                payload: resolved.variant.payload.clone(),
            },
            impression_data: resolved.impression_data,
            impressionData: resolved.impression_data,
        })
        .sorted_by_key(|toggle| toggle.name.clone())
        .collect::<Vec<EvaluatedToggle>>();
    FrontendResult { toggles }
}
