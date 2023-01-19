use std::path::PathBuf;

use clap::{Args, Parser};

#[derive(clap::ValueEnum, Debug, Clone)]
pub enum EdgeMode {
    Offline,
}

#[derive(Parser, Debug, Clone)]
pub struct CliArgs {
    #[clap(flatten)]
    pub http: HttpServerArgs,

    #[arg(value_enum)]
    pub mode: EdgeMode,

    #[clap(short, long, env)]
    pub bootstrap_file: Option<PathBuf>,
}

impl CliArgs {
    pub fn http_server_tuple(&self) -> (String, u16) {
        (self.http.interface.clone(), self.http.port)
    }

    pub fn https_server_tuple(&self) -> (String, u16) {
        (self.http.interface.clone(), self.http.tls.tls_server_port)
    }
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
