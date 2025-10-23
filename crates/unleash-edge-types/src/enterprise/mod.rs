use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EnterpriseEdgeLicenseState {
    Valid,
    Invalid,
    Expired,
    Undetermined,
}
