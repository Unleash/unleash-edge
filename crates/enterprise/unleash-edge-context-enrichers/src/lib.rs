mod child;
mod command;
mod context_enricher;
mod driver;
mod pool;
mod protocol;
mod worker;

const MAX_SCHEDULED_JOBS: usize = 32;

pub use command::EnricherError;
pub use context_enricher::ContextEnricher;
pub use pool::WorkerPool;
