use std::{collections::HashMap, num::NonZeroU32, path::PathBuf, sync::Arc, time::Duration};
use tracing::warn;
use unleash_types::client_features::Context;

use crate::{EnricherError, WorkerPool};

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

    pub async fn enrich_or_original(
        &self,
        context: Context,
        headers: HashMap<String, String>,
        timeout: Duration,
    ) -> Context {
        let Some(worker_pool) = self.worker_pool.as_ref() else {
            return context;
        };

        match worker_pool
            .request_enrichment(context.clone(), headers, timeout)
            .await
        {
            Ok(enriched_context) => enriched_context,
            Err(error) => {
                warn!(
                    "Failed to enrich frontend context, falling back to original context: {error}"
                );
                context
            }
        }
    }
}
