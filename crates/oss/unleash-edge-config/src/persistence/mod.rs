use crate::persistence::redis::RedisOpts;
use std::path::PathBuf;

pub mod redis;
#[cfg(feature = "s3-persistence")]
pub mod s3;

#[derive(Debug, Clone, Default)]
pub enum PersistenceConfig {
    S3(S3Opts),
    Redis(RedisOpts),
    File(FileOpts),
    #[default]
    None,
}

#[derive(Debug, Clone)]
pub struct S3Opts {
    pub bucket_name: String,
}

#[derive(Debug, Clone)]
pub struct FileOpts {
    pub folder: PathBuf,
}
