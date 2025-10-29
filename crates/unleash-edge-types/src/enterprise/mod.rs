use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};

use serde::{Deserialize, Serialize};
use tracing::warn;
use utoipa::ToSchema;

#[repr(u8)]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema, Copy)]
pub enum LicenseState {
    Valid = 0,
    Invalid = 1,
    Expired = 2,
}

#[derive(Debug, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HeartbeatResponse {
    pub edge_license_state: LicenseState,
}

#[derive(Clone)]
pub struct ApplicationLicenseState(Arc<AtomicU8>);

impl ApplicationLicenseState {
    pub fn new(v: LicenseState) -> Self {
        Self(Arc::new(AtomicU8::new(v as u8)))
    }

    pub fn get(&self) -> LicenseState {
        match self.0.load(Ordering::Acquire) {
            0 => LicenseState::Valid,
            1 => LicenseState::Invalid,
            3 => LicenseState::Expired,
            _ => {
                warn!("Invalid license state detected, defaulting to Invalid");
                LicenseState::Invalid
            }
        }
    }

    pub fn set(&self, v: LicenseState) {
        self.0.store(v as u8, Ordering::Release)
    }
}
