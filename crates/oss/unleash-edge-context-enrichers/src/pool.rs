use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use tokio::{
    sync::{Mutex, Semaphore},
    time::Duration,
};

use crate::worker::{EnrichmentError, NodeWorker, default_worker_script_path};

#[derive(Clone)]
pub struct WorkerPool {
    inner: Arc<PoolInner>,
}

struct PoolInner {
    workers: Vec<WorkerSlot>,
    next_worker: Mutex<usize>,
    global_capacity: Semaphore,
}

struct WorkerSlot {
    worker: NodeWorker,
    in_flight: AtomicUsize,
    max_in_flight: usize,
}

impl WorkerPool {
    pub async fn start(
        worker_count: usize,
        max_in_flight_per_worker: usize,
        script_path: PathBuf,
    ) -> Result<Self, EnrichmentError> {
        Self::start_with_worker_script(
            worker_count,
            max_in_flight_per_worker,
            default_worker_script_path(),
            script_path,
        )
        .await
    }

    pub async fn start_with_worker_script(
        worker_count: usize,
        max_in_flight_per_worker: usize,
        worker_script: PathBuf,
        script_path: PathBuf,
    ) -> Result<Self, EnrichmentError> {
        if worker_count == 0 {
            return Err(EnrichmentError::WorkerUnavailable(
                "worker_count must be greater than zero".into(),
            ));
        }
        if max_in_flight_per_worker == 0 {
            return Err(EnrichmentError::WorkerUnavailable(
                "max_in_flight_per_worker must be greater than zero".into(),
            ));
        }

        let worker_script = absolutize(worker_script)?;
        let customer_script = absolutize(script_path)?;
        let mut workers = Vec::with_capacity(worker_count);

        for worker_id in 0..worker_count {
            let worker =
                NodeWorker::start(worker_id, worker_script.clone(), customer_script.clone())
                    .await?;
            workers.push(WorkerSlot {
                worker,
                in_flight: AtomicUsize::new(0),
                max_in_flight: max_in_flight_per_worker,
            });
        }

        Ok(Self {
            inner: Arc::new(PoolInner {
                workers,
                next_worker: Mutex::new(0),
                global_capacity: Semaphore::new(worker_count * max_in_flight_per_worker),
            }),
        })
    }

    pub async fn execute(
        &self,
        context: serde_json::Value,
        timeout: Duration,
    ) -> Result<serde_json::Value, EnrichmentError> {
        let _permit = self
            .inner
            .global_capacity
            .acquire()
            .await
            .map_err(|_| EnrichmentError::WorkerUnavailable("pool semaphore closed".into()))?;
        let index = self.select_worker().await?;
        let _load_guard = LoadGuard {
            slot: &self.inner.workers[index],
        };

        self.inner.workers[index]
            .worker
            .execute(context, timeout)
            .await
    }

    pub fn worker_pids(&self) -> Vec<u32> {
        self.inner
            .workers
            .iter()
            .map(|slot| slot.worker.pid())
            .collect()
    }

    async fn select_worker(&self) -> Result<usize, EnrichmentError> {
        let mut next_worker = self.inner.next_worker.lock().await;
        let worker_count = self.inner.workers.len();
        let start = *next_worker;
        let mut best = None;

        for offset in 0..worker_count {
            let index = (start + offset) % worker_count;
            let slot = &self.inner.workers[index];
            let load = slot.in_flight.load(Ordering::SeqCst);

            if load >= slot.max_in_flight {
                continue;
            }

            match best {
                None => best = Some((index, load)),
                Some((_, best_load)) if load < best_load => best = Some((index, load)),
                _ => {}
            }
        }

        let Some((index, _)) = best else {
            return Err(EnrichmentError::WorkerUnavailable(
                "all workers are at capacity".into(),
            ));
        };

        self.inner.workers[index]
            .in_flight
            .fetch_add(1, Ordering::SeqCst);
        *next_worker = (index + 1) % worker_count;
        Ok(index)
    }
}

struct LoadGuard<'a> {
    slot: &'a WorkerSlot,
}

impl Drop for LoadGuard<'_> {
    fn drop(&mut self) {
        self.slot.in_flight.fetch_sub(1, Ordering::SeqCst);
    }
}

fn absolutize(path: PathBuf) -> Result<PathBuf, EnrichmentError> {
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}
