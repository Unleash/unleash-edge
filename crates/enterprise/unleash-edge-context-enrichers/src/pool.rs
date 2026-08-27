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

    #[cfg_attr(not(test), expect(dead_code))]
    async fn request_enrichment(
        &self,
        context: Context,
        headers: HashMap<String, String>,
        timeout: Duration,
    ) -> Result<Context, EnricherError> {
        let deadline = Instant::now() + timeout;

        let _permit = timeout_at(deadline, self.inner.job_slots.acquire())
            .await
            .map_err(|_| EnricherError::IOError("Worker response timed out".to_string()))?
            .map_err(|_| {
                EnricherError::UnexpectedShutdown("Worker pool is shutting down".to_string())
            })?;
        let worker_index = self.select_next_worker().await?;

        let _load_guard = LoadGuard {
            slot: &self.inner.worker_slots[worker_index],
        };

        self.inner.worker_slots[worker_index]
            .worker
            .request_enrichment(
                context,
                headers,
                deadline.saturating_duration_since(Instant::now()),
            )
            .await
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
            Err(EnricherError::IOError("All workers are busy".to_string()))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::WorkerCommand;
    use tokio::sync::mpsc::{Receiver, channel};
    use tokio::time;

    fn worker_slot(worker_id: u32) -> (WorkerSlot, Receiver<WorkerCommand>) {
        let (command_tx, command_rx) = channel(MAX_SCHEDULED_JOBS);
        (
            WorkerSlot {
                worker: NodeWorkerController::from_command_tx(worker_id, command_tx),
                jobs_inflight: AtomicUsize::new(0),
            },
            command_rx,
        )
    }

    fn pool_with_slots(worker_slots: Vec<WorkerSlot>, max_jobs: usize) -> WorkerPool {
        WorkerPool {
            inner: Arc::new(InnerPool {
                worker_slots,
                next_worker_index: Mutex::new(0),
                job_slots: Semaphore::new(max_jobs),
            }),
        }
    }

    async fn next_command(command_rx: &mut Receiver<WorkerCommand>) -> WorkerCommand {
        timeout_at(Instant::now() + Duration::from_secs(1), command_rx.recv())
            .await
            .expect("timed out waiting for worker command")
            .expect("worker command channel closed")
    }

    #[tokio::test]
    async fn select_next_worker_rotates_across_idle_workers() {
        let (first_worker, _first_rx) = worker_slot(0);
        let (second_worker, _second_rx) = worker_slot(1);
        let (third_worker, _third_rx) = worker_slot(2);
        let pool = pool_with_slots(vec![first_worker, second_worker, third_worker], 3);

        let first = pool.select_next_worker().await.expect("select failed");
        pool.inner.worker_slots[first]
            .jobs_inflight
            .fetch_sub(1, Ordering::SeqCst);
        let second = pool.select_next_worker().await.expect("select failed");
        pool.inner.worker_slots[second]
            .jobs_inflight
            .fetch_sub(1, Ordering::SeqCst);
        let third = pool.select_next_worker().await.expect("select failed");

        assert_eq!((first, second, third), (0, 1, 2));
    }

    #[tokio::test]
    async fn select_next_worker_skips_workers_at_capacity() {
        let (first_worker, _first_rx) = worker_slot(0);
        first_worker
            .jobs_inflight
            .store(MAX_SCHEDULED_JOBS, Ordering::SeqCst);
        let (second_worker, _second_rx) = worker_slot(1);
        let pool = pool_with_slots(vec![first_worker, second_worker], 2);

        let selected_worker = pool.select_next_worker().await.expect("select failed");

        assert_eq!(selected_worker, 1);
    }

    #[tokio::test]
    async fn request_enrichment_dispatches_to_selected_worker_and_releases_load() {
        let (worker, mut command_rx) = worker_slot(0);
        let pool = pool_with_slots(vec![worker], 1);
        let request = tokio::spawn({
            let pool = WorkerPool {
                inner: Arc::clone(&pool.inner),
            };
            async move {
                pool.request_enrichment(
                    Context::default(),
                    HashMap::from([("x-test".to_string(), "true".to_string())]),
                    Duration::from_secs(1),
                )
                .await
            }
        });

        match next_command(&mut command_rx).await {
            WorkerCommand::Execute {
                id,
                headers,
                respond_to,
                ..
            } => {
                assert_eq!(id, 0);
                assert_eq!(headers.get("x-test").map(String::as_str), Some("true"));
                let context = Context {
                    user_id: Some("pooled-user".to_string()),
                    ..Default::default()
                };
                respond_to
                    .send(Ok(context))
                    .expect("request receiver dropped");
            }
            WorkerCommand::Shutdown => panic!("unexpected shutdown command"),
        }

        let response = request
            .await
            .expect("request task panicked")
            .expect("request failed");

        assert_eq!(response.user_id.as_deref(), Some("pooled-user"));
        assert_eq!(
            pool.inner.worker_slots[0]
                .jobs_inflight
                .load(Ordering::SeqCst),
            0
        );
        assert_eq!(pool.inner.job_slots.available_permits(), 1);
    }

    #[tokio::test]
    async fn request_enrichment_times_out_when_pool_capacity_is_exhausted() {
        let (worker, _command_rx) = worker_slot(0);
        let pool = pool_with_slots(vec![worker], 0);

        let error = pool
            .request_enrichment(
                Context::default(),
                HashMap::new(),
                Duration::from_millis(20),
            )
            .await
            .expect_err("request should time out waiting for pool capacity");

        match error {
            EnricherError::IOError(message) => assert_eq!(message, "Worker response timed out"),
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[tokio::test]
    async fn request_enrichment_timeout_budget_includes_pool_queue_wait() {
        let (worker, mut command_rx) = worker_slot(0);
        let pool = pool_with_slots(vec![worker], 1);
        let permit = pool
            .inner
            .job_slots
            .acquire()
            .await
            .expect("pool semaphore closed");

        let request = tokio::spawn({
            let pool = WorkerPool {
                inner: Arc::clone(&pool.inner),
            };
            async move {
                pool.request_enrichment(
                    Context::default(),
                    HashMap::new(),
                    Duration::from_millis(50),
                )
                .await
            }
        });

        time::sleep(Duration::from_millis(40)).await;
        drop(permit);

        match next_command(&mut command_rx).await {
            WorkerCommand::Execute {
                deadline,
                respond_to,
                ..
            } => {
                time::sleep(Duration::from_millis(30)).await;
                if deadline <= Instant::now() {
                    respond_to
                        .send(Err(EnricherError::IOError(
                            "Worker response timed out".to_string(),
                        )))
                        .expect("request receiver dropped");
                } else {
                    respond_to
                        .send(Ok(Context::default()))
                        .expect("request receiver dropped");
                }
            }
            WorkerCommand::Shutdown => panic!("unexpected shutdown command"),
        }

        let error = request
            .await
            .expect("request task panicked")
            .expect_err("request should observe original timeout budget");

        match error {
            EnricherError::IOError(message) => assert_eq!(message, "Worker response timed out"),
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[tokio::test]
    async fn request_enrichment_preserves_worker_error_kind() {
        let (worker, mut command_rx) = worker_slot(0);
        let pool = pool_with_slots(vec![worker], 1);
        let request = tokio::spawn({
            let pool = WorkerPool {
                inner: Arc::clone(&pool.inner),
            };
            async move {
                pool.request_enrichment(Context::default(), HashMap::new(), Duration::from_secs(1))
                    .await
            }
        });

        match next_command(&mut command_rx).await {
            WorkerCommand::Execute { respond_to, .. } => {
                respond_to
                    .send(Err(EnricherError::ScriptError(
                        "script blew up".to_string(),
                    )))
                    .expect("request receiver dropped");
            }
            WorkerCommand::Shutdown => panic!("unexpected shutdown command"),
        }

        let error = request
            .await
            .expect("request task panicked")
            .expect_err("request should fail");

        match error {
            EnricherError::ScriptError(message) => assert_eq!(message, "script blew up"),
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
