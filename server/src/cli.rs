use std::path::PathBuf;

use clap::{ArgGroup, Args, Parser, Subcommand};

#[derive(Subcommand, Debug, Clone)]
pub enum EdgeMode {
    /// Run in offline mode
    Offline(OfflineArgs),
    /// Run in edge mode
    Edge(EdgeArgs),
}

pub enum EdgeArg {
    Redis(String),
    Dynamo(String),
}

impl From<EdgeArgs> for EdgeArg {
    fn from(value: EdgeArgs) -> Self {
        if let Some(redis_url) = value.redis_url {
            return EdgeArg::Redis(redis_url);
        };
        if let Some(dynamo_url) = value.dynamo_url {
            return EdgeArg::Dynamo(dynamo_url);
        }
        panic!("Unknown argument for edge type"); //This shouldn't be reachable without programmer error, that's what it's for
    }
}

#[derive(Args, Debug, Clone)]
#[command(group(
    ArgGroup::new("data-provider")
        .required(true)
        .args(["redis_url", "dynamo_url"]),
))]
pub struct EdgeArgs {
    #[clap(short, long, env)]
    pub redis_url: Option<String>,
    #[clap(short, long, env)]
    pub dynamo_url: Option<String>,
}

#[derive(Args, Debug, Clone)]
pub struct OfflineArgs {
    #[clap(short, long, env)]
    pub bootstrap_file: Option<PathBuf>,
    #[clap(short, long, env, value_delimiter = ',')]
    pub client_keys: Vec<String>,
}

#[derive(Parser, Debug, Clone)]
pub struct CliArgs {
    #[clap(flatten)]
    pub http: HttpServerArgs,

    #[command(subcommand)]
    pub mode: EdgeMode,
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
