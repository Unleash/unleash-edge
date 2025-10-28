use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::EdgeResult;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum LicenseStateResponse {
    Valid,
    Invalid,
    Expired,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LicenseState {
    Valid,
    Invalid,
    Expired,
    Undetermined,
}

impl From<EdgeResult<LicenseStateResponse>> for LicenseState {
    fn from(result: EdgeResult<LicenseStateResponse>) -> Self {
        match result {
            Ok(LicenseStateResponse::Valid) => LicenseState::Valid,
            Ok(LicenseStateResponse::Invalid) => LicenseState::Invalid,
            Ok(LicenseStateResponse::Expired) => LicenseState::Expired,
            Err(_) => LicenseState::Undetermined,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]

pub struct HeartbeatResponse {
    pub edge_license_state: LicenseStateResponse,
}
