use actix_web::{
    body::MessageBody,
    dev::{ServiceRequest, ServiceResponse},
    web::Data,
    HttpResponse,
};

use crate::types::{EdgeSource, EdgeToken};
use tokio::sync::{mpsc::Sender, RwLock};

pub async fn validate_token(
    token: EdgeToken,
    provider: Data<RwLock<dyn EdgeSource>>,
    sender: Data<Sender<EdgeToken>>,
    req: ServiceRequest,
    srv: crate::middleware::as_async_middleware::Next<impl MessageBody + 'static>,
) -> Result<ServiceResponse<impl MessageBody>, actix_web::Error> {
    let res = if provider
        .read()
        .await
        .secret_is_valid(token.token.as_str(), sender.into_inner())
        .await?
    {
        srv.call(req).await?.map_into_left_body()
    } else {
        req.into_response(HttpResponse::Forbidden().finish())
            .map_into_right_body()
    };
    Ok(res)
}
