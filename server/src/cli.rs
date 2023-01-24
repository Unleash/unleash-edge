use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Subcommand, Debug, Clone)]
pub enum EdgeMode {
    Offline(OfflineArgs),
}

#[derive(Args, Debug, Clone)]
pub struct OfflineArgs {
    #[clap(short, long, env)]
    pub bootstrap_file: Option<PathBuf>,
    #[clap(short, long, env)]
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
