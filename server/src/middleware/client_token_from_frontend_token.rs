use crate::{types::{EdgeToken, TokenType, TokenValidationStatus, ServiceAccountToken}, http::unleash_client::UnleashClient};
use actix_web::{
    body::MessageBody,
    dev::{ServiceRequest, ServiceResponse},
    web::Data,
};
use dashmap::DashMap;


pub async fn client_token_from_frontend_token(
    token: EdgeToken, 
    req: ServiceRequest,
    srv: crate::middleware::as_async_middleware::Next<impl MessageBody + 'static>
) -> Result<ServiceResponse<impl MessageBody>, actix_web::Error> {
    if let Some(TokenType::Frontend) = token.token_type {
        if TokenValidationStatus::Validated == token.status {
            if let Some(_service_account_token) = req.app_data::<ServiceAccountToken>() {
                if let Some(unleash_client) = req.app_data::<Data<UnleashClient>>().cloned() {
                    let client_token = unleash_client.into_inner().get_client_token_for_unhydrated_frontend_token(token.clone()).await?;
                    if let Some(token_cache) = req.app_data::<Data<DashMap<String, EdgeToken>>>() {
                        token_cache.insert(client_token.token.clone(), client_token.clone());
                    }
                }
            } 
        }
    }
    srv.call(req).await
}