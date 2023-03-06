use std::path::PathBuf;

use clap::{ArgGroup, Args, Parser, Subcommand};

#[derive(Subcommand, Debug, Clone)]
pub enum EdgeMode {
    /// Run in edge mode
    Edge(EdgeArgs),
    /// Run in offline mode
    Offline(OfflineArgs),
}

pub enum Persister {
    Redis(String),
    File(PathBuf),
    None,
}

impl From<EdgeArgs> for Persister {
    fn from(value: EdgeArgs) -> Self {
        if let Some(redis_url) = value.redis_url {
            return Persister::Redis(redis_url);
        };

        if let Some(backup_folder) = value.backup_folder {
            return Persister::File(backup_folder);
        };

        Persister::None
    }
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

    #[clap(short, long, env)]
    pub redis_url: Option<String>,

    /// Edge can periodically persist its state to disk. Tell us where?
    #[clap(long, env)]
    pub backup_folder: Option<PathBuf>,
    /// How often should we post metrics upstream?
    #[clap(short, long, env, default_value_t = 60)]
    pub metrics_interval_seconds: u64,
    /// How long between each refresh for a token
    #[clap(short, long, env, default_value_t = 10)]
    pub features_refresh_interval_seconds: i64,

    /// Get data for these client tokens at startup. Hot starts your feature cache
    #[clap(short, long, env)]
    pub tokens: Vec<String>,
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
