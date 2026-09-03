use http::HeaderMap;
use std::{num::NonZeroU32, path::PathBuf, sync::Arc, time::Duration};
use tracing::{info, warn};
use unleash_types::client_features::Context;

use crate::{EnricherError, WorkerPool, serializable_header::SerializableHeaders};

#[derive(Clone)]
pub struct ContextEnricher {
    worker_pool: Option<Arc<WorkerPool>>,
}

impl ContextEnricher {
    pub fn disabled() -> Self {
        Self { worker_pool: None }
    }

    pub async fn start(
        worker_count: NonZeroU32,
        script_path: PathBuf,
    ) -> Result<Self, EnricherError> {
        Ok(Self {
            worker_pool: Some(Arc::new(
                WorkerPool::start(worker_count, script_path).await?,
            )),
        })
    }

    pub async fn try_enrich(
        &self,
        context: &Context,
        headers: &HeaderMap,
        timeout: Duration,
    ) -> Option<Context> {
        let worker_pool = self.worker_pool.as_ref()?;

        match worker_pool
            .request_enrichment(context, SerializableHeaders(headers), timeout)
            .await
        {
            Ok(enriched_context) => Some(enriched_context),
            Err(error) => {
                match error {
                    EnricherError::Timeout(message) => {
                        info!(
                            "Context enrichment timed out, falling back to original context: {message}"
                        );
                    }
                    _ => {
                        warn!(
                            "Failed to enrich frontend context, falling back to original context: {error}"
                        );
                    }
                }
                None
            }
        }
    }
}
