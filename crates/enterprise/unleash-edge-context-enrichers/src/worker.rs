use std::{
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
use tokio::{
    sync::{
        mpsc::{Sender, channel},
        oneshot::{self},
    },
    time::{self, Instant},
};
use tracing::error;
use unleash_types::client_features::Context;

use crate::{
    MAX_SCHEDULED_JOBS,
    child::spawn_node_child_process,
    command::{EnricherError, WorkerCommand},
    driver::driver_loop,
    protocol::{EnrichmentRequest, SerializedEnrichmentRequest},
    serializable_header::SerializableHeaders,
};

pub struct NodeWorkerController {
    #[expect(dead_code)]
    worker_id: u32,
    next_request_id: AtomicU64,
    command_tx: Sender<WorkerCommand>,
}

impl NodeWorkerController {
    #[cfg(test)]
    pub(crate) fn from_command_tx(worker_id: u32, command_tx: Sender<WorkerCommand>) -> Self {
        NodeWorkerController {
            worker_id,
            next_request_id: AtomicU64::new(0),
            command_tx,
        }
    }

    pub async fn start(worker_id: u32, enricher_script: &Path) -> Result<Self, EnricherError> {
        let mut child = spawn_node_child_process(worker_id, enricher_script).await?;

        let (command_tx, mut command_rx) = channel(MAX_SCHEDULED_JOBS);

        let enricher_script = enricher_script.to_path_buf();

        tokio::spawn(async move {
            loop {
                match driver_loop(worker_id, &mut command_rx, &mut child).await {
                    Ok(()) => {
                        break;
                    }
                    Err(error) => {
                        error!("[node-worker {worker_id}] driver failed; restarting: {error}");
                    }
                }

                // Small sleep to prevent a broken setup from going psycho on re-spawning processes
                // This is pretty bad. Edge under load is going to drop a bunch of enricher requests here
                // the alternative is a hot re-spawn loop but that's going to melt Edge in a different way
                // This isn't normal failure - something is broken and needs to be logged and raised anyway
                tokio::time::sleep(Duration::from_millis(10)).await;
                child = match spawn_node_child_process(worker_id, &enricher_script).await {
                    Ok(child) => child,
                    Err(error) => {
                        error!("[node-worker {worker_id}] failed to restart child: {error}");
                        continue;
                    }
                };
            }
        });

        Ok(NodeWorkerController {
            worker_id,
            next_request_id: AtomicU64::new(0),
            command_tx,
        })
    }

    pub async fn request_enrichment(
        &self,
        context: Context,
        headers: SerializableHeaders<'_>,
        job_timeout: Duration,
    ) -> Result<Context, EnricherError> {
        let (respond_to, read_response) = oneshot::channel();
        let deadline = Instant::now() + job_timeout;
        let id = self.next_request_id.fetch_add(1, Ordering::SeqCst);
        let request = SerializedEnrichmentRequest::try_from(EnrichmentRequest {
            id,
            context,
            headers,
        })
        .map_err(|e| {
            EnricherError::ProtocolError(format!("Could not serialize message to enricher: {e}"))
        })?;

        let command = WorkerCommand::Execute {
            id,
            request,
            deadline,
            respond_to,
        };

        time::timeout_at(deadline, self.command_tx.send(command))
            .await
            .map_err(|_| EnricherError::Timeout("Worker response timed out".to_string()))?
            .map_err(|e| {
                EnricherError::IOError(format!("Failed to send command to worker: {}", e))
            })?;

        match read_response.await {
            Ok(result) => result,
            Err(_) => Err(EnricherError::IOError(
                "Worker response channel closed unexpectedly".to_string(),
            )),
        }
    }

    #[expect(dead_code)]
    pub async fn shutdown(&self) -> Result<(), EnricherError> {
        self.command_tx
            .send(WorkerCommand::Shutdown)
            .await
            .map_err(|e| {
                EnricherError::IOError(format!("Failed to send shutdown command to worker: {}", e))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use http::HeaderMap;

    #[tokio::test]
    async fn request_enrichment_times_out_when_scheduler_queue_is_full() {
        let (command_tx, _command_rx) = channel(1);
        command_tx
            .send(WorkerCommand::Shutdown)
            .await
            .expect("failed to fill scheduler queue");
        let worker = NodeWorkerController::from_command_tx(1, command_tx);
        let headers = HeaderMap::new();

        let error = worker
            .request_enrichment(
                Context::default(),
                SerializableHeaders(&headers),
                Duration::from_millis(20),
            )
            .await
            .expect_err("request should time out waiting for scheduler queue capacity");

        match error {
            EnricherError::Timeout(message) => assert_eq!(message, "Worker response timed out"),
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
