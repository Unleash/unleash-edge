#[derive(Debug, Clone)]
pub struct PrometheusConfig {
    pub remote_write_url: String,
    pub push_interval: u64,
    pub username: Option<String>,
    pub password: Option<String>,
}
