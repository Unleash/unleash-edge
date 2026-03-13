use std::time::Duration;

#[derive(Debug, Clone)]
pub struct RedisOpts {
    pub redis_password: Option<String>,
    pub redis_mode: RedisMode,
    pub redis_url: Option<Vec<String>>,
    pub read_timeout: Duration,
    pub write_timeout: Duration,
}

#[derive(Debug, Clone, Eq, PartialEq, PartialOrd, Ord)]
pub enum RedisMode {
    Single(String),
    Cluster(Vec<String>),
}
