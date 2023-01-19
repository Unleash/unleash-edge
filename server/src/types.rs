use actix_web::web::Json;
use unleash_types::client_features::ClientFeatures;

use crate::error::EdgeError;

pub type EdgeJsonResult<T> = Result<Json<T>, EdgeError>;

pub trait FeaturesProvider {
    fn get_client_features(&self) -> ClientFeatures;
}
