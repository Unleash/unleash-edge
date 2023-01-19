use actix_web::web::Json;

use crate::error::EdgeError;

pub type EdgeJsonResult<T> = Result<Json<T>, EdgeError>;
