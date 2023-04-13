use std::path::PathBuf;

use clap::{ArgGroup, Args, Parser, Subcommand};
use iter_tools::Itertools;

#[derive(Subcommand, Debug, Clone)]
pub enum EdgeMode {
    /// Run in edge mode
    Edge(EdgeArgs),
    /// Run in offline mode
    Offline(OfflineArgs),
}
#[derive(Args, Debug, Clone)]
#[command(group(
    ArgGroup::new("data-provider")
        .args(["redis_url", "backup_folder"]),
))]
pub struct EdgeArgs {
    /// Where is your upstream URL. Remember, this is the URL to your instance, without any trailing /api suffix
    #[clap(short, long, env)]
    pub upstream_url: String,

    /// A URL pointing to a running Redis instance. Edge will use this instance to persist feature and token data and read this back after restart. Mutually exclusive with the --backup-folder option
    #[clap(short, long, env)]
    pub redis_url: Option<String>,

    /// A path to a local folder. Edge will write feature and token data to disk in this folder and read this back after restart. Mutually exclusive with the --redis-url option
    #[clap(short, long, env)]
    pub backup_folder: Option<PathBuf>,
    /// How often should we post metrics upstream?
    #[clap(short, long, env, default_value_t = 60)]
    pub metrics_interval_seconds: u64,
    /// How long between each refresh for a token
    #[clap(short, long, env, default_value_t = 10)]
    pub features_refresh_interval_seconds: u64,

    /// How long between each revalidation of a token
    #[clap(long, env, default_value_t = 3600)]
    pub token_revalidation_interval_seconds: u64,

    /// Get data for these client tokens at startup. Hot starts your feature cache
    #[clap(short, long, env, value_delimiter = ',')]
    pub tokens: Vec<String>,

    /// Expects curl header format (-H <HEADERNAME>: <HEADERVALUE>)
    /// for instance `-H X-Api-Key: mysecretapikey`
    #[clap(short = 'H', long, env, value_delimiter = ',', value_parser = string_to_header_tuple)]
    pub custom_client_headers: Vec<(String, String)>,
}

pub fn string_to_header_tuple(s: &str) -> Result<(String, String), String> {
    let format_message =
        "Please pass headers in the format <headername>:<headervalue>".to_string();
    if s.contains(':') {
        if let Some((header_name, header_value)) = s.split(':').collect_tuple() {
            Ok((
                header_name.trim().to_string(),
                header_value.trim().to_string(),
            ))
        } else {
            Err(format_message)
        }
    } else {
        Err(format_message)
    }
}

#[derive(Args, Debug, Clone)]
pub struct OfflineArgs {
    #[clap(short, long, env)]
    pub bootstrap_file: Option<PathBuf>,
    #[clap(short, long, env, value_delimiter = ',')]
    pub tokens: Vec<String>,
}

#[derive(Parser, Debug, Clone)]
pub struct CliArgs {
    #[clap(flatten)]
    pub http: HttpServerArgs,

    #[command(subcommand)]
    pub mode: EdgeMode,

    /// Instance id. Used for metrics reporting.
    #[clap(long, env, default_value_t = ulid::Ulid::new().to_string())]
    pub instance_id: String,

    /// App name. Used for metrics reporting.
    #[clap(short, long, env, default_value = "unleash-edge")]
    pub app_name: String,
}

#[derive(Args, Debug, Clone)]
pub struct TlsOptions {
    /// Should we bind TLS
    #[clap(env, long, default_value_t = false)]
    pub tls_enable: bool,
    /// Server key to use for TLS
    #[clap(env, long)]
    pub tls_server_key: Option<PathBuf>,
    #[clap(env, long)]
    /// Server Cert to use for TLS
    pub tls_server_cert: Option<PathBuf>,
    /// Port to listen for https connection on (will use the interfaces already defined)
    #[clap(env, long, default_value_t = 3043)]
    pub tls_server_port: u16,
}

#[derive(Args, Debug, Clone)]
pub struct HttpServerArgs {
    /// Which port should this server listen for HTTP traffic on
    #[clap(short, long, env, default_value_t = 3063)]
    pub port: u16,
    /// Which interfaces should this server listen for HTTP traffic on
    #[clap(short, long, env, default_value = "0.0.0.0")]
    pub interface: String,

    /// How many workers should be started to handle requests.
    /// Defaults to number of physical cpus
    #[clap(short, long, env, default_value_t = num_cpus::get_physical())]
    pub workers: usize,

    #[clap(flatten)]
    pub tls: TlsOptions,
}

impl HttpServerArgs {
    pub fn http_server_tuple(&self) -> (String, u16) {
        (self.interface.clone(), self.port)
    }

    pub fn https_server_tuple(&self) -> (String, u16) {
        (self.interface.clone(), self.tls.tls_server_port)
    }
}

#[cfg(test)]
mod tests {
    use crate::cli::{CliArgs, EdgeMode};
    use clap::Parser;

    #[test]
    pub fn can_parse_multiple_client_headers() {
        let args = vec![
            "unleash-edge",
            "edge",
            "-u http://localhost:4242",
            r#"-H Authorization: abc123"#,
            r#"-H X-Api-Key: mysecret"#,
        ];
        let args = CliArgs::parse_from(args);
        match args.mode {
            EdgeMode::Edge(args) => {
                let client_headers = args.custom_client_headers;
                assert_eq!(client_headers.len(), 2);
                let auth = client_headers.get(0).unwrap();
                assert_eq!(auth.0, "Authorization");
                assert_eq!(auth.1, "abc123");
                let api_key = client_headers.get(1).unwrap();
                assert_eq!(api_key.0, "X-Api-Key");
                assert_eq!(api_key.1, "mysecret")
            }
            EdgeMode::Offline(_) => unreachable!(),
        }
    }

    #[test]
    pub fn can_parse_comma_separated_client_headers() {
        let args = vec![
            "unleash-edge",
            "edge",
            "-u http://localhost:4242",
            r#"-H Authorization: abc123,X-Api-Key: mysecret"#,
        ];
        let args = CliArgs::parse_from(args);
        match args.mode {
            EdgeMode::Edge(args) => {
                let client_headers = args.custom_client_headers;
                assert_eq!(client_headers.len(), 2);
                let auth = client_headers.get(0).unwrap();
                assert_eq!(auth.0, "Authorization");
                assert_eq!(auth.1, "abc123");
                let api_key = client_headers.get(1).unwrap();
                assert_eq!(api_key.0, "X-Api-Key");
                assert_eq!(api_key.1, "mysecret")
            }
            EdgeMode::Offline(_) => unreachable!(),
        }
    }
}
