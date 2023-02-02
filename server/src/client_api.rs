use crate::types::{EdgeJsonResult, EdgeSource, EdgeToken};
use actix_web::get;
use actix_web::web::{self, Json};
use unleash_types::client_features::ClientFeatures;

#[get("/client/features")]
async fn features(
    edge_token: EdgeToken,
    features_source: web::Data<dyn EdgeSource>,
) -> EdgeJsonResult<ClientFeatures> {
    let client_features = features_source.get_client_features(&edge_token).await?;
    Ok(Json(client_features))
}

pub fn configure_client_api(cfg: &mut web::ServiceConfig) {
    cfg.service(features);
}
