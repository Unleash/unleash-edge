#[derive(Copy, Debug, Clone, Eq, PartialEq, PartialOrd, Ord)]
pub enum RedisMode {
    Single,
    Cluster,
}

impl From<unleash_edge_cli::RedisMode> for RedisMode {
    fn from(value: unleash_edge_cli::RedisMode) -> Self {
        match value {
            unleash_edge_cli::RedisMode::Cluster => RedisMode::Cluster,
            unleash_edge_cli::RedisMode::Single => RedisMode::Single,
        }
    }
}

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
