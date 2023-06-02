use crate::{types::{EdgeToken, TokenType, TokenValidationStatus, ServiceAccountToken, EdgeResult}, http::{unleash_client::UnleashClient, feature_refresher::FeatureRefresher}, tokens::cache_key};
use actix_web::{
    body::MessageBody,
    dev::{ServiceRequest, ServiceResponse},
    web::Data,
};
use unleash_yggdrasil::EngineState;
use dashmap::DashMap;
use tracing::{instrument, info, debug};


pub fn have_data_for_fe_token(req: &ServiceRequest, token: &EdgeToken) -> bool {
    if let Some(engine_cache) = req.app_data::<Data<DashMap<String, EngineState>>>() {
        engine_cache.contains_key(&cache_key(&token))
    } else {
        false
    }

}

pub async fn create_client_token_for_fe_token(req: &ServiceRequest, token: &EdgeToken) -> EdgeResult<()> {
    if let Some(_service_account_token) = req.app_data::<Data<ServiceAccountToken>>() {
        debug!("Had a service account token");
        if let Some(feature_refresher) = req.app_data::<Data<FeatureRefresher>>().cloned() {
            debug!("Had a feature refresher");
            feature_refresher.create_client_token_for_fe_token(token.clone()).await;
        }
    } else {
        debug!("Did not have a service account token, will end up returning 511");
    } 
    Ok(())
}

#[instrument(skip(req, srv))]
pub async fn client_token_from_frontend_token(
    token: EdgeToken, 
    req: ServiceRequest,
    srv: crate::middleware::as_async_middleware::Next<impl MessageBody + 'static>
) -> Result<ServiceResponse<impl MessageBody>, actix_web::Error> {
    if let Some(token_cache) = req.app_data::<Data<DashMap<String, EdgeToken>>>() {
        debug!("Had a token cache");
        if let Some(fe_token) = token_cache.get(&token.token) {
            debug!("Token got extracted to {:#?}", fe_token.value().clone());
            if fe_token.status == TokenValidationStatus::Validated {
                create_client_token_for_fe_token(&req, &fe_token).await?;
            }
        }
    }
    srv.call(req).await
}