use std::fmt::{Display, Formatter};
use std::path::PathBuf;

use clap::{ArgGroup, Args, Parser, Subcommand, ValueEnum};

#[derive(Subcommand, Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum EdgeMode {
    /// Run in edge mode
    Edge(EdgeArgs),
    /// Run in offline mode
    Offline(OfflineArgs),
    /// Perform a health check against a running edge instance
    Health(HealthCheckArgs),
}

#[derive(ValueEnum, Debug, Clone)]
pub enum RedisScheme {
    Redis,
    Rediss,
    RedisUnix,
    Unix,
}

impl Display for RedisScheme {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            RedisScheme::Redis => write!(f, "redis"),
            RedisScheme::Rediss => write!(f, "rediss"),
            RedisScheme::RedisUnix => write!(f, "redis+unix"),
            RedisScheme::Unix => write!(f, "unix"),
        }
    }
}
#[derive(Args, Debug, Clone)]
pub struct RedisArgs {
    #[clap(long, env)]
    pub redis_url: Option<String>,
    #[clap(long, env)]
    pub redis_password: Option<String>,
    #[clap(long, env)]
    pub redis_username: Option<String>,
    #[clap(long, env)]
    pub redis_port: Option<u16>,
    #[clap(long, env)]
    pub redis_host: Option<String>,
    #[clap(long, env, default_value_t = false)]
    pub redis_secure: bool,
    #[clap(long, env, default_value_t = RedisScheme::Redis, value_enum)]
    pub redis_scheme: RedisScheme,
}

impl RedisArgs {
    pub fn to_url(&self) -> Option<String> {
        self.redis_url
            .clone()
            .map(|url| {
                reqwest::Url::parse(&url).unwrap_or_else(|_| panic!("Failed to create url from REDIS_URL: {}, REDIS_USERNAME: {} and REDIS_PASSWORD: {}", self.redis_url.clone().unwrap_or("NO_URL".into()), self.redis_username.clone().unwrap_or("NO_USERNAME_SET".into()), self.redis_password.is_some()))
            })
            .or_else(|| self.redis_host.clone().map(|host| {
                reqwest::Url::parse(format!("{}://{}", self.redis_scheme, &host).as_str()).expect("Failed to parse hostname from REDIS_HOSTNAME or --redis-hostname parameters")
            }))
            .map(|base| {
                let mut base_url = base;
                if self.redis_password.is_some() {
                    base_url.set_password(Some(&self.redis_password.clone().unwrap())).expect("Failed to set password");
                }
                if self.redis_username.is_some() {
                    base_url.set_username(&self.redis_username.clone().unwrap()).expect("Failed to set username");
                }
                base_url.set_port(self.redis_port).expect("Failed to set port");
                base_url
            }).map(|almost_finished_url| {
                let mut base_url = almost_finished_url;
                if self.redis_secure {
                    base_url.set_scheme("rediss").expect("Failed to set redis scheme");
                }
                base_url
        }).map(|f| f.to_string())
    }
}
#[derive(Args, Debug, Clone)]
pub struct ClientIdentity {
    /// Client certificate chain in PEM encoded X509 format with the leaf certificate first.
    /// The certificate chain should contain any intermediate certificates that should be sent to clients to allow them to build a chain to a trusted root
    #[clap(long, env)]
    pub pkcs8_client_certificate_file: Option<PathBuf>,
    /// Client key is a PEM encoded PKCS#8 formatted private key for the leaf certificate
    #[clap(long, env)]
    pub pkcs8_client_key_file: Option<PathBuf>,
    /// Identity file in pkcs12 format. Typically this file has a pfx extension
    #[clap(long, env)]
    pub pkcs12_identity_file: Option<PathBuf>,
    #[clap(long, env)]
    /// Passphrase used to unlock the pkcs12 file
    pub pkcs12_passphrase: Option<String>,
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
    #[clap(flatten)]
    pub redis: Option<RedisArgs>,

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

    /// Get data for these client tokens at startup. Accepts comma-separated list of tokens. Hot starts your feature cache
    #[clap(short, long, env, value_delimiter = ',')]
    pub tokens: Vec<String>,

    /// Expects curl header format (-H <HEADERNAME>: <HEADERVALUE>)
    /// for instance `-H X-Api-Key: mysecretapikey`
    #[clap(short = 'H', long, env, value_delimiter = ',', value_parser = string_to_header_tuple)]
    pub custom_client_headers: Vec<(String, String)>,

    /// If set to true, we will skip SSL verification when connecting to the upstream Unleash server
    #[clap(short, long, env, default_value_t = false)]
    pub skip_ssl_verification: bool,

    #[clap(flatten)]
    pub client_identity: Option<ClientIdentity>,

    /// Extra certificate passed to the client for building its trust chain. Needs to be in PEM format (crt or pem extensions usually are)
    #[clap(long, env)]
    pub upstream_certificate_file: Option<PathBuf>,

    /// Service account token. Used to create client tokens if receiving a frontend token we don't have data for
    #[clap(long, global = true, env)]
    pub service_account_token: Option<String>,
}

pub fn string_to_header_tuple(s: &str) -> Result<(String, String), String> {
    let format_message = "Please pass headers in the format <headername>:<headervalue>".to_string();
    if s.contains(':') {
        if let Some((header_name, header_value)) = s.split_once(':') {
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

#[derive(Args, Debug, Clone)]
pub struct HealthCheckArgs {
    /// Where the instance you want to health check is running
    #[clap(short, long, env, default_value = "http://localhost:3063")]
    pub edge_url: String,

    /// If you're hosting Edge using a self-signed TLS certificate use this to tell healthcheck about your CA
    #[clap(short, long, env)]
    pub ca_certificate_file: Option<PathBuf>,
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

    #[arg(long, hide = true, global = true)]
    pub markdown_help: bool,
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
pub struct ExperimentalArgs {
    /// Exposes the api/client/features endpoint for POST requests. This may be removed in a future release
    #[clap(env, long, default_value_t = false)]
    pub enable_post_features: bool,
}

#[derive(Args, Debug, Clone)]
pub struct HttpServerArgs {
    /// Which port should this server listen for HTTP traffic on
    #[clap(short, long, env, default_value_t = 3063)]
    pub port: u16,
    /// Which interfaces should this server listen for HTTP traffic on
    #[clap(short, long, env, default_value = "0.0.0.0")]
    pub interface: String,
    /// Which base path should this server listen for HTTP traffic on
    #[clap(short, long, env, default_value = "")]
    pub base_path: String,

    /// How many workers should be started to handle requests.
    /// Defaults to number of physical cpus
    #[clap(short, long, env, global=true, default_value_t = num_cpus::get_physical())]
    pub workers: usize,

    #[clap(flatten)]
    pub experimental: ExperimentalArgs,

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
            EdgeMode::Health(_) => unreachable!(),
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
            EdgeMode::Health(_) => unreachable!(),
        }
    }

    #[test]
    pub fn can_handle_colons_in_header_value() {
        let args = vec![
            "unleash-edge",
            "edge",
            "-u http://localhost:4242",
            r#"-H Authorization: test:test.secret"#,
        ];
        let args = CliArgs::parse_from(args);
        match args.mode {
            EdgeMode::Edge(args) => {
                let client_headers = args.custom_client_headers;
                assert_eq!(client_headers.len(), 1);
                let auth = client_headers.get(0).unwrap();
                assert_eq!(auth.0, "Authorization");
                assert_eq!(auth.1, "test:test.secret");
            }
            EdgeMode::Offline(_) => unreachable!(),
            EdgeMode::Health(_) => unreachable!(),
        }
    }

    #[test]
    pub fn can_create_redis_url_from_redis_url_argument() {
        let args = vec![
            "unleash-edge",
            "edge",
            "-u http://localhost:4242",
            "--redis-url",
            "redis://localhost/redis",
        ];
        let args = CliArgs::parse_from(args);
        match args.mode {
            EdgeMode::Edge(args) => {
                let redis_url = args.redis.unwrap().to_url();
                assert!(redis_url.is_some());
                assert_eq!(redis_url.unwrap(), "redis://localhost/redis");
            }
            _ => unreachable!(),
        }
    }

    #[test]
    pub fn can_create_redis_url_from_more_specific_redis_arguments() {
        let args = vec![
            "unleash-edge",
            "edge",
            "-u http://localhost:4242",
            "--redis-host",
            "localhost",
            "--redis-username",
            "redis",
            "--redis-password",
            "password",
            "--redis-port",
            "6389",
            "--redis-scheme",
            "rediss",
        ];
        let args = CliArgs::parse_from(args);
        match args.mode {
            EdgeMode::Edge(args) => {
                let redis_url = args.redis.unwrap().to_url();
                assert!(redis_url.is_some());
                assert_eq!(redis_url.unwrap(), "rediss://redis:password@localhost:6389");
            }
            _ => unreachable!(),
        }
    }

    #[test]
    pub fn can_combine_redis_url_with_username_and_password() {
        let args = vec![
            "unleash-edge",
            "edge",
            "-u http://localhost:4242",
            "--redis-url",
            "redis://localhost",
            "--redis-username",
            "redis",
            "--redis-password",
            "password",
        ];
        let args = CliArgs::parse_from(args);
        match args.mode {
            EdgeMode::Edge(args) => {
                let redis_url = args.redis.unwrap().to_url();
                assert!(redis_url.is_some());
                assert_eq!(redis_url.unwrap(), "redis://redis:password@localhost");
            }
            _ => unreachable!(),
        }
    }

    #[test]
    pub fn setting_redis_secure_to_true_overrides_set_scheme() {
        let args = vec![
            "unleash-edge",
            "edge",
            "-u http://localhost:4242",
            "--redis-url",
            "redis://localhost",
            "--redis-username",
            "redis",
            "--redis-password",
            "password",
            "--redis-secure",
        ];
        let args = CliArgs::parse_from(args);
        match args.mode {
            EdgeMode::Edge(args) => {
                let redis_url = args.redis.unwrap().to_url();
                assert!(redis_url.is_some());
                assert_eq!(redis_url.unwrap(), "rediss://redis:password@localhost");
            }
            _ => unreachable!(),
        }
    }

    #[test]
    pub fn setting_secure_to_true_overrides_the_scheme_for_detailed_arguments_as_well() {
        let args = vec![
            "unleash-edge",
            "edge",
            "-u http://localhost:4242",
            "--redis-host",
            "localhost",
            "--redis-username",
            "redis",
            "--redis-password",
            "password",
            "--redis-port",
            "6389",
            "--redis-secure",
        ];
        let args = CliArgs::parse_from(args);
        match args.mode {
            EdgeMode::Edge(args) => {
                let redis_url = args.redis.unwrap().to_url();
                assert!(redis_url.is_some());
                assert_eq!(redis_url.unwrap(), "rediss://redis:password@localhost:6389");
            }
            _ => unreachable!(),
        }
    }
}
