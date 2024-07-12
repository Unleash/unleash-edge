use std::fmt::{Display, Formatter};
use std::net::IpAddr;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

use cidr::{Ipv4Cidr, Ipv6Cidr};
use clap::{ArgGroup, Args, Parser, Subcommand, ValueEnum};

use crate::error;

#[derive(Subcommand, Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum EdgeMode {
    /// Run in edge mode
    Edge(EdgeArgs),
    /// Run in offline mode
    Offline(OfflineArgs),
    /// Perform a health check against a running edge instance
    Health(HealthCheckArgs),
    /// Perform a ready check against a running edge instance
    Ready(ReadyCheckArgs),
}

#[derive(ValueEnum, Debug, Clone)]
pub enum RedisScheme {
    Tcp,
    Tls,
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
            RedisScheme::Tcp => write!(f, "redis"),
            RedisScheme::Tls => write!(f, "rediss"),
        }
    }
}
#[derive(Copy, Debug, Clone, Eq, PartialEq, PartialOrd, Ord, ValueEnum)]
pub enum RedisMode {
    Single,
    Cluster,
}

#[derive(Args, Debug, Clone)]
pub struct RedisArgs {
    #[clap(long, env, value_delimiter = ',')]
    pub redis_url: Option<Vec<String>>,
    #[clap(long, env, value_enum, default_value_t = RedisMode::Single)]
    pub redis_mode: RedisMode,
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
    /// Timeout (in milliseconds) for waiting for a successful connection to redis, when restoring
    #[clap(long, env, default_value_t = 2000)]
    pub redis_read_connection_timeout_milliseconds: u64,
    /// Timeout (in milliseconds) for waiting for a successful connection to redis when persisting
    #[clap(long, env, default_value_t = 2000)]
    pub redis_write_connection_timeout_milliseconds: u64,
}

impl RedisArgs {
    pub fn to_url(&self) -> Option<String> {
        self.redis_url
            .clone()
            .map(|url| {
                reqwest::Url::parse(&url[0]).unwrap_or_else(|_| panic!("Failed to create url from REDIS_URL: {:?}, REDIS_USERNAME: {} and REDIS_PASSWORD: {}", self.redis_url.clone().unwrap_or(vec!["NO_URL".into()]), self.redis_username.clone().unwrap_or("NO_USERNAME_SET".into()), self.redis_password.is_some()))
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
    pub fn read_timeout(&self) -> std::time::Duration {
        Duration::from_millis(self.redis_read_connection_timeout_milliseconds)
    }
    pub fn write_timeout(&self) -> std::time::Duration {
        Duration::from_millis(self.redis_write_connection_timeout_milliseconds)
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

    /// Timeout for requests to the upstream server
    #[clap(long, env, default_value_t = 5)]
    pub upstream_request_timeout: i64,

    /// Socket timeout for requests to upstream
    #[clap(long, env, default_value_t = 5)]
    pub upstream_socket_timeout: i64,

    /// A URL pointing to a running Redis instance. Edge will use this instance to persist feature and token data and read this back after restart. Mutually exclusive with the --backup-folder option
    #[clap(flatten)]
    pub redis: Option<RedisArgs>,

    /// Token header to use for both edge authorization and communication with the upstream server.
    #[clap(long, env, global = true, default_value = "Authorization")]
    pub token_header: TokenHeader,

    /// If set to true, Edge starts with strict behavior. Strict behavior means that Edge will refuse tokens outside the scope of the startup tokens
    #[clap(long, env, default_value_t = false)]
    pub strict: bool,

    /// If set to true, Edge starts with dynamic behavior. Dynamic behavior means that Edge will accept tokens outside the scope of the startup tokens
    #[clap(long, env, default_value_t = false, conflicts_with = "strict")]
    pub dynamic: bool,
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
    /// The file to load our features from. This data will be loaded at startup
    #[clap(short, long, env)]
    pub bootstrap_file: Option<PathBuf>,
    /// Tokens that should be allowed to connect to Edge. Supports a comma separated list or multiple instances of the `--tokens` argument
    #[clap(short, long, env, value_delimiter = ',')]
    pub tokens: Vec<String>,
    /// The interval in seconds between reloading the bootstrap file. Disabled if unset or 0
    #[clap(short, long, env, default_value_t = 0)]
    pub reload_interval: u64,
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

#[derive(Args, Debug, Clone)]
pub struct TokenHeader {
    /// Token header to use for edge authorization.
    #[clap(long, env, global = true, default_value = "Authorization")]
    pub token_header: String,
}

impl FromStr for TokenHeader {
    type Err = clap::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let token_header = s.to_owned();
        Ok(TokenHeader { token_header })
    }
}

#[derive(Args, Debug, Clone)]
pub struct ReadyCheckArgs {
    /// Where the instance you want to health check is running
    #[clap(short, long, env, default_value = "http://localhost:3063")]
    pub edge_url: String,

    /// If you're hosting Edge using a self-signed TLS certificate use this to tell the readychecker about your CA
    #[clap(short, long, env)]
    pub ca_certificate_file: Option<PathBuf>,
}

#[derive(Debug, Clone, ValueEnum)]
pub enum LogFormat {
    Plain,
    Json,
    Pretty,
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

    #[clap(flatten)]
    pub trust_proxy: TrustProxy,

    /// Set this flag to true if you want to disable /api/proxy/all and /api/frontend/all
    /// Because returning all toggles regardless of their state is a potential security vulnerability, these endpoints can be disabled
    #[clap(long, env, default_value_t = false, global = true)]
    pub disable_all_endpoint: bool,

    /// Timeout for requests to Edge
    #[clap(long, env, default_value_t = 5)]
    pub edge_request_timeout: u64,

    /// Which log format should Edge use
    #[clap(short, long, env, global = true, value_enum, default_value_t = LogFormat::Plain)]
    pub log_format: LogFormat,

    /// token header to use for edge authorization.
    #[clap(long, env, global = true, default_value = "Authorization")]
    pub token_header: TokenHeader,
}

#[derive(Args, Debug, Clone)]
pub struct TlsOptions {
    /// Should we bind TLS
    #[clap(env, long, default_value_t = false)]
    pub tls_enable: bool,
    /// Server key to use for TLS - Needs to be a path to a file
    #[clap(env, long)]
    pub tls_server_key: Option<PathBuf>,
    #[clap(env, long)]
    /// Server Cert to use for TLS - Needs to be a path to a file
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
    /// Which base path should this server listen for HTTP traffic on
    #[clap(short, long, env, default_value = "")]
    pub base_path: String,

    /// How many workers should be started to handle requests.
    /// Defaults to number of physical cpus
    #[clap(short, long, env, global=true, default_value_t = num_cpus::get_physical())]
    pub workers: usize,

    #[clap(flatten)]
    pub tls: TlsOptions,
}

#[derive(Debug, Clone)]
pub enum NetworkAddr {
    Ip(IpAddr),
    CidrIpv4(Ipv4Cidr),
    CidrIpv6(Ipv6Cidr),
}

#[derive(Args, Debug, Clone)]
pub struct TrustProxy {
    /// By enabling the trust proxy option. Unleash Edge will have knowledge that it's sitting behind a proxy and that the X-Forward-\* header fields may be trusted, which otherwise may be easily spoofed.
    /// Edge will use this to populate its context's  remoteAddress field
    /// If you need to only trust specific ips or CIDR, enable this flag and then set `--proxy-trusted-servers`
    #[clap(long, env, global = true)]
    pub trust_proxy: bool,

    /// Tells Unleash Edge which servers to trust the X-Forwarded-For. Accepts explicit Ip addresses or Cidrs (127.0.0.1/16). Accepts a comma separated list or multiple instances of the flag.
    /// E.g `--proxy-trusted-servers "127.0.0.1,192.168.0.1"` and `--proxy-trusted-servers 127.0.0.1 --proxy-trusted-servers 192.168.0.1` are equivalent
    #[clap(long, env, value_delimiter = ',', global = true, value_parser = ip_or_cidr)]
    pub proxy_trusted_servers: Vec<NetworkAddr>,
}

pub fn ip_or_cidr(s: &str) -> Result<NetworkAddr, String> {
    match IpAddr::from_str(s) {
        Ok(ipaddr) => Ok(NetworkAddr::Ip(ipaddr)),
        Err(_e) => match Ipv4Cidr::from_str(s) {
            Ok(ipv4cidr) => Ok(NetworkAddr::CidrIpv4(ipv4cidr)),
            Err(_e) => match Ipv6Cidr::from_str(s) {
                Ok(ipv6cidr) => Ok(NetworkAddr::CidrIpv6(ipv6cidr)),
                Err(_e) => Err(error::TRUST_PROXY_PARSE_ERROR.into()),
            },
        },
    }
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
    use clap::Parser;
    use tracing::info;
    use tracing_test::traced_test;

    use crate::cli::{CliArgs, EdgeMode, NetworkAddr};
    use crate::error;

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
                let auth = client_headers.first().unwrap();
                assert_eq!(auth.0, "Authorization");
                assert_eq!(auth.1, "abc123");
                let api_key = client_headers.get(1).unwrap();
                assert_eq!(api_key.0, "X-Api-Key");
                assert_eq!(api_key.1, "mysecret")
            }
            _ => unreachable!(),
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
                let auth = client_headers.first().unwrap();
                assert_eq!(auth.0, "Authorization");
                assert_eq!(auth.1, "abc123");
                let api_key = client_headers.get(1).unwrap();
                assert_eq!(api_key.0, "X-Api-Key");
                assert_eq!(api_key.1, "mysecret")
            }
            _ => unreachable!(),
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
                let auth = client_headers.first().unwrap();
                assert_eq!(auth.0, "Authorization");
                assert_eq!(auth.1, "test:test.secret");
            }
            _ => unreachable!(),
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

    #[test]
    pub fn setting_scheme_to_tls_uses_the_rediss_protocol() {
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
            "tls",
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
    pub fn setting_scheme_to_tcp_uses_the_redis_protocol() {
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
            "tcp",
        ];
        let args = CliArgs::parse_from(args);
        match args.mode {
            EdgeMode::Edge(args) => {
                let redis_url = args.redis.unwrap().to_url();
                assert!(redis_url.is_some());
                assert_eq!(redis_url.unwrap(), "redis://redis:password@localhost:6389");
            }
            _ => unreachable!(),
        }
    }

    #[test]
    #[traced_test]
    pub fn proxy_trusted_servers_accept_both_ipv4_and_ipv6() {
        let args = vec![
            "unleash-edge",
            "edge",
            "-u http://localhost:4242",
            "--trust-proxy",
            "--proxy-trusted-servers",
            "192.168.0.1",
            "--proxy-trusted-servers",
            "::1",
        ];
        let args = CliArgs::parse_from(args);
        assert!(args.trust_proxy.trust_proxy);
        info!("{:?}", args.trust_proxy.proxy_trusted_servers);
        assert_eq!(args.trust_proxy.proxy_trusted_servers.len(), 2);
        let first = args.trust_proxy.proxy_trusted_servers.first().unwrap();
        if let NetworkAddr::Ip(ip_addr) = first {
            assert!(ip_addr.is_ipv4());
        } else {
            unreachable!()
        }
        let second = args.trust_proxy.proxy_trusted_servers.get(1).unwrap();
        if let NetworkAddr::Ip(ip_addr) = second {
            assert!(ip_addr.is_ipv6());
        } else {
            unreachable!()
        }
    }

    #[test]
    pub fn proxy_trusted_servers_accept_both_ipv4_and_ipv6_cidr_addresses() {
        let args = vec![
            "unleash-edge",
            "edge",
            "-u http://localhost:4242",
            "--trust-proxy",
            "--proxy-trusted-servers",
            "192.168.0.0/16",
            "--proxy-trusted-servers",
            "2001:db8:1234::/48",
        ];
        let args = CliArgs::parse_from(args);
        info!("{:?}", args.trust_proxy.proxy_trusted_servers);
        assert_eq!(args.trust_proxy.proxy_trusted_servers.len(), 2);
        let first = args.trust_proxy.proxy_trusted_servers.first().unwrap();
        if let NetworkAddr::CidrIpv4(cidr) = first {
            assert_eq!(cidr.network_length(), 16);
        } else {
            unreachable!()
        }
        let second = args.trust_proxy.proxy_trusted_servers.get(1).unwrap();
        if let NetworkAddr::CidrIpv6(ip_addr) = second {
            assert_eq!(ip_addr.network_length(), 48);
        } else {
            unreachable!()
        }
    }

    #[test]
    pub fn incorrect_trusted_servers_format_yields_error_message() {
        let args = vec![
            "unleash-edge",
            "edge",
            "-u http://localhost:4242",
            "--trust-proxy",
            "--proxy-trusted-servers",
            "192.168.0.0/125",
        ];
        let args = CliArgs::try_parse_from(args);
        assert!(args.is_err());
        assert!(args
            .err()
            .unwrap()
            .to_string()
            .contains(error::TRUST_PROXY_PARSE_ERROR));
    }
}
