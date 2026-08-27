use crate::{MAX_SCHEDULED_JOBS, command::EnricherError, worker::NodeWorkerController};
use std::{
    collections::HashMap,
    num::NonZeroU32,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::sync::{Mutex, Semaphore};
use tokio::time::{Duration, Instant, timeout_at};
use unleash_types::client_features::Context;

struct WorkerSlot {
    worker: NodeWorkerController,
    jobs_inflight: AtomicUsize,
}

pub struct WorkerPool {
    inner: Arc<InnerPool>,
}

struct InnerPool {
    worker_slots: Vec<WorkerSlot>,
    next_worker_index: Mutex<usize>,
    job_slots: Semaphore,
}

struct LoadGuard<'a> {
    slot: &'a WorkerSlot,
}

impl Drop for LoadGuard<'_> {
    fn drop(&mut self) {
        self.slot.jobs_inflight.fetch_sub(1, Ordering::SeqCst);
    }
}

impl WorkerPool {
    #[expect(dead_code)]
    async fn start(worker_count: NonZeroU32, script_path: PathBuf) -> Result<Self, EnricherError> {
        let mut workers = vec![];

        for worker_id in 0..worker_count.get() {
            let worker =
                NodeWorkerController::start(worker_id, &as_absolute_path(&script_path)?).await?;
            workers.push(WorkerSlot {
                jobs_inflight: AtomicUsize::new(0),
                worker,
            });
        }

        Ok(Self {
            inner: Arc::new(InnerPool {
                worker_slots: workers,
                next_worker_index: Mutex::new(0),
                // We're limited to the max number of jobs per worker * number of workers - and we can express that
                // in a concurrency primitive to apply bounded back pressure to the caller if all workers are busy
                job_slots: Semaphore::new(MAX_SCHEDULED_JOBS * worker_count.get() as usize),
            }),
        })
    }

    #[expect(dead_code)]
    async fn request_enrichment(
        &self,
        context: Context,
        headers: HashMap<String, String>,
        timeout: Duration,
    ) -> Result<Context, EnricherError> {
        let deadline = Instant::now() + timeout;

        let _permit = timeout_at(deadline, self.inner.job_slots.acquire())
            .await
            .map_err(|_| {
                EnricherError::UnexpectedShutdown("Worker pool is shutting down".to_string())
            })?;
        let worker_index = self.select_next_worker().await?;

        let _load_guard = LoadGuard {
            slot: &self.inner.worker_slots[worker_index],
        };

        self.inner.worker_slots[worker_index]
            .worker
            .request_enrichment(context, headers, timeout)
            .await
            .map_err(|e| {
                EnricherError::UnexpectedShutdown(format!(
                    "Worker {} failed to process request: {e}",
                    worker_index
                ))
            })
    }

    async fn select_next_worker(&self) -> Result<usize, EnricherError> {
        // This is a bit subtle and I don't know that I've made the right choice here, only that one needed to be made
        // This algorithm spreads load equally across workers by always rotating out the next worker index on selection.
        // Under high load this doesn't matter because we'll pick the lowest load worker, but under low load this means
        // that we spread the jobs around the whole pool rather than ping ponging between worker 1 and worker 2. Does this
        // matter? Dunno, might be net negative for cache locality, might be net positive for GC pressure
        let mut next_worker_index = self.inner.next_worker_index.lock().await;
        let total_workers = self.inner.worker_slots.len();

        let start = *next_worker_index;
        let mut idle_worker = None;

        for index in 0..total_workers {
            let worker_index = (start + index) % total_workers;
            let worker_slot = &self.inner.worker_slots[worker_index];
            let worker_load = worker_slot.jobs_inflight.load(Ordering::SeqCst);

            if worker_load >= MAX_SCHEDULED_JOBS {
                // because jobs are async behind the scenes we can't guarantee that jobs
                // will be completed in the order they were started. If we're here
                // a single worker has gotten unlucky and is too busy to accept a new job
                // so we skip it and try the next worker. It'll free up eventually
                continue;
            }

            match idle_worker {
                None => idle_worker = Some((worker_index, worker_load)),
                Some((_, best_load)) if worker_load < best_load => {
                    idle_worker = Some((worker_index, worker_load))
                }
                _ => {
                    // Oops, even busier than the last guy, don't consider me please
                }
            }
        }

        if let Some((selected_worker_index, _)) = idle_worker {
            self.inner.worker_slots[selected_worker_index]
                .jobs_inflight
                .fetch_add(1, Ordering::SeqCst);

            *next_worker_index = (selected_worker_index + 1) % total_workers;

            Ok(selected_worker_index)
        } else {
            Err(EnricherError::UnexpectedShutdown(
                "All workers are busy".to_string(),
            ))
        }
    }
}

fn as_absolute_path(path: &PathBuf) -> Result<PathBuf, EnricherError> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()
            .map_err(|e| {
                EnricherError::StartupFailure(format!("Failed to get current directory: {}", e))
            })?
            .join(path))
    }
}
