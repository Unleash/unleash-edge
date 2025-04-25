use serde::{Deserialize, Serialize};

use crate::EdgeToken;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EdgeTokens {
    pub tokens: Vec<EdgeToken>,
}
