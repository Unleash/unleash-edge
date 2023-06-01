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

pub async fn create_client_token_for_fe_token(req: &ServiceRequest, token: &EdgeToken) -> EdgeResult<Option<EdgeToken>> {
    if let Some(_service_account_token) = req.app_data::<Data<ServiceAccountToken>>() {
        debug!("Had a service account token");
        if let Some(feature_refresher) = req.app_data::<Data<FeatureRefresher>>().cloned() {
            debug!("Had a feature refresher. Creating token");
            let unleash_client = feature_refresher.into_inner().unleash_client.clone();
            Ok(unleash_client.get_client_token_for_unhydrated_frontend_token(token.clone()).await.ok())
        } else {
            Ok(None)
        }
    } else {
        debug!("Did not have a service account token. Can't create tokens");
        Ok(None)
    }
    
}

#[instrument(skip(req, srv))]
pub async fn client_token_from_frontend_token(
    token: EdgeToken, 
    req: ServiceRequest,
    srv: crate::middleware::as_async_middleware::Next<impl MessageBody + 'static>
) -> Result<ServiceResponse<impl MessageBody>, actix_web::Error> {
    info!("Unwrapping tokens for enabling");
    if let Some(token_cache) = req.app_data::<Data<DashMap<String, EdgeToken>>>() {
        debug!("Had a token cache");
        if let Some(fe_token) = token_cache.get(&token.token) {
            debug!("Token got extracted to {:#?}", fe_token.value().clone());
            if fe_token.status == TokenValidationStatus::Validated {
                if !have_data_for_fe_token(&req, &fe_token.value().clone()) {
                    debug!("Did not have data for fe token");
                    if let Some(client_token) = create_client_token_for_fe_token(&req, &fe_token).await? {
                        debug!("Created client token {client_token:?}");
                        token_cache.insert(client_token.token.clone(), client_token.clone());
                        if let Some(feature_refresher) = req.app_data::<Data<FeatureRefresher>>() {
                            info!("Registering for refresh");
                            feature_refresher.register_token_for_refresh(client_token, None).await;
                        }
                    }
                }     
            }
        }
    }
    srv.call(req).await
}