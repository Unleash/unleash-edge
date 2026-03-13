use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;
use ulid::Ulid;
use unleash_edge_cli::{CliArgs, ClientIdentity, EdgeArgs};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientMetaInformation {
    pub app_name: String,
    pub instance_id: Ulid,
    pub connection_id: Ulid,
}

impl Default for ClientMetaInformation {
    fn default() -> Self {
        let ulid = Ulid::new();
        Self {
            app_name: "unleash-edge".to_string(),
            instance_id: ulid,
            connection_id: ulid,
        }
    }
}

impl From<&CliArgs> for ClientMetaInformation {
    fn from(value: &CliArgs) -> Self {
        let id = Ulid::new();
        Self {
            app_name: value.app_name.clone(),
            instance_id: id,
            connection_id: id,
        }
    }
}

#[cfg_attr(test, derive(Default))]
#[derive(Clone, Debug)]
pub struct HttpClientOpts {
    pub skip_ssl_verification: bool,
    pub client_identity: Option<ClientIdentity>,
    pub upstream_certificate_file: Option<PathBuf>,
    pub connect_timeout: Duration,
    pub socket_timeout: Duration,
    pub keep_alive_timeout: Duration,
    pub client_meta_information: ClientMetaInformation,
}

impl HttpClientOpts {
    pub fn from_edge_args_and_meta_information(
        args: &EdgeArgs,
        meta: ClientMetaInformation,
    ) -> Self {
        Self {
            skip_ssl_verification: args.skip_ssl_verification,
            client_identity: args.client_identity.clone(),
            upstream_certificate_file: args.upstream_certificate_file.clone(),
            connect_timeout: Duration::from_secs(args.upstream_request_timeout),
            socket_timeout: Duration::from_secs(args.upstream_socket_timeout),
            keep_alive_timeout: Duration::from_secs(args.client_keepalive_timeout),
            client_meta_information: meta,
        }
    }
}
