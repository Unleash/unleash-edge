#[derive(Debug, Clone, Default)]
pub enum LogFormat {
    #[default]
    Plain,
    Json,
    Pretty,
}

impl From<unleash_edge_cli::LogFormat> for LogFormat {
    fn from(value: unleash_edge_cli::LogFormat) -> Self {
        match value {
            unleash_edge_cli::LogFormat::Plain => LogFormat::Plain,
            unleash_edge_cli::LogFormat::Json => LogFormat::Json,
            unleash_edge_cli::LogFormat::Pretty => LogFormat::Pretty,
        }
    }
}
