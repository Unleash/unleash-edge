use std::time::Duration;

use awc::Client;

use crate::types::BuildInfo;

const UNLEASH_APPNAME_HEADER: &str = "UNLEASH-APPNAME";
const UNLEASH_INSTANCE_ID_HEADER: &str = "UNLEASH-INSTANCEID";
const UNLEASH_CLIENT_SPEC_HEADER: &str = "Unleash-Client-Spec";

pub struct UnleashClient {
    server_url: String,
    backing_client: Client,
}

impl UnleashClient {
    pub fn new(server_url: String, instance_id_opt: Option<String>) -> Self {
        let build_info = BuildInfo::new();
        let instance_id = instance_id_opt.unwrap_or(build_info.instance_id());
        Self {
            server_url,
            backing_client: awc::Client::builder()
                .add_default_header((UNLEASH_APPNAME_HEADER, build_info.app_name))
                .add_default_header((UNLEASH_INSTANCE_ID_HEADER, instance_id))
                .add_default_header(
                    (UNLEASH_CLIENT_SPEC_HEADER, "4.2.2"), // yggdrasil::CLIENT_SPEC_VERSION).into(),
                )
                .timeout(Duration::from_secs(5))
                .finish(),
        }
    }
}
