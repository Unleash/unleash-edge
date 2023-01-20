use crate::types::{EdgeJsonResult, FeaturesProvider};
use actix_web::get;
use actix_web::web::{self, Json};
use unleash_types::client_features::ClientFeatures;

#[get("/client/features")]
async fn features(
    features_source: web::Data<dyn FeaturesProvider>,
) -> EdgeJsonResult<ClientFeatures> {
    let client_features = features_source.get_client_features();
    Ok(Json(client_features))
}

pub fn configure_client_api(cfg: &mut web::ServiceConfig) {
    cfg.service(features);
}
