use std::{collections::HashMap, time::Duration};
use tokio::{
    io::AsyncWriteExt,
    process::ChildStdin,
    sync::{mpsc::Receiver, oneshot::Sender as OneShotSender},
    time::{self, Instant},
};
use tracing::error;
use unleash_types::client_features::Context;

use crate::{
    child::RunningNodeChild,
    command::{EnricherError, WorkerCommand, WorkerEvent},
    protocol::EnrichmentRequest,
};

const PENDING_RESPONSE_EXPIRY_INTERVAL: Duration = Duration::from_millis(10);

pub(crate) struct PendingResponse {
    deadline: Instant,
    respond_to: OneShotSender<Result<Context, EnricherError>>,
}

pub(crate) async fn driver_loop(
    worker_id: u32,
    command_rx: &mut Receiver<WorkerCommand>,
    child: &mut RunningNodeChild,
) -> Result<(), EnricherError> {
    let mut pending_responses = HashMap::new();
    let mut pending_expiry = time::interval(PENDING_RESPONSE_EXPIRY_INTERVAL);
    pending_expiry.set_missed_tick_behavior(time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            command = command_rx.recv() => {
                match command {
                    Some(WorkerCommand::Execute { id, context, headers, deadline, respond_to }) => {
                        // Our queue is backed up. Badly. And trying to add a job to the queue is
                        // exceeding out timeout. Soooo... no point in sending then because we no longer care
                        if deadline <= Instant::now() {
                            let _ = respond_to.send(Err(EnricherError::IOError(
                                "Worker response timed out".to_string(),
                            )));
                            continue;
                        }

                        if let Err(error) =
                            send_request(&mut child.child_input, id, context, headers).await
                        {
                            let _ = respond_to.send(Err(error.clone()));
                            fail_pending_responses(&mut pending_responses, error.clone());
                            return Err(error);
                        }
                        pending_responses.insert(id, PendingResponse {
                            deadline,
                            respond_to,
                        });
                    }
                    Some(WorkerCommand::Shutdown) | None => {
                        fail_pending_responses(
                            &mut pending_responses,
                            EnricherError::IOError(
                                "Worker is shutting down, this job will not be served".to_string(),
                            ),
                        );
                        return child.terminate().await;
                    }
                }
            }
            message = child.child_output.recv() => {
                match message {
                    Some(WorkerEvent::Response(response)) => {
                        if let Some(pending_response) = pending_responses.remove(&response.id) {
                            let _ = pending_response.respond_to.send(response.outcome.map_err(EnricherError::ScriptError));
                        } else {
                            error!(
                                "[node-worker {worker_id} pid={}] received response for unknown request id {}",
                                child.pid,
                                response.id
                            );
                        }
                    }
                    Some(WorkerEvent::WorkerError(line)) => {
                        let error = EnricherError::ProtocolError(format!("child process sent unparsable message: {line}"));
                        fail_pending_responses(&mut pending_responses, error.clone());
                        return Err(error);
                    }
                    None => {
                        let error = EnricherError::UnexpectedShutdown(
                            "child process stdout closed".to_string(),
                        );
                        fail_pending_responses(&mut pending_responses, error.clone());
                        return Err(error);
                    }
                }
            }
            status = child.child.wait() => {
                let message = match status {
                    Ok(status) => {
                        format!("child process exited with status: {status}")
                    }
                    Err(error) => {
                        format!("child process wait failed: {error}")
                    }
                };

                let error = EnricherError::UnexpectedShutdown(message);
                fail_pending_responses(&mut pending_responses, error.clone());
                return Err(error);
            }
            _ = pending_expiry.tick(), if !pending_responses.is_empty() => {
                expire_pending_responses(&mut pending_responses);
            }
        }
    }
}

fn fail_pending_responses(
    pending_responses: &mut HashMap<u64, PendingResponse>,
    error: EnricherError,
) {
    for (_id, pending_response) in pending_responses.drain() {
        let _ = pending_response.respond_to.send(Err(error.clone()));
    }
}

fn expire_pending_responses(pending_responses: &mut HashMap<u64, PendingResponse>) {
    let now = Instant::now();
    let expired_ids = pending_responses
        .iter()
        .filter_map(|(id, pending_response)| (pending_response.deadline <= now).then_some(*id))
        .collect::<Vec<_>>();

    for id in expired_ids {
        if let Some(pending_response) = pending_responses.remove(&id) {
            let _ = pending_response.respond_to.send(Err(EnricherError::IOError(
                "Worker response timed out".to_string(),
            )));
        }
    }
}

async fn send_request(
    stdin: &mut ChildStdin,
    id: u64,
    context: Context,
    headers: HashMap<String, String>,
) -> Result<(), EnricherError> {
    let request = EnrichmentRequest {
        id,
        context,
        headers,
    };
    // This isn't possible in the sense that it requires that we have a fallible serialize implementation
    // We don't have that, there's no reason to have that and it's a bunch of work to do so for no reason.
    let mut line = serde_json::to_vec(&request).map_err(|e| {
        EnricherError::ProtocolError(format!("Could not serialize message to enricher: {e}"))
    })?;
    line.push(b'\n');
    stdin
        .write_all(&line)
        .await
        .map_err(|e| EnricherError::IOError(format!("Could not send message to enricher: {e}")))?;
    stdin
        .flush()
        .await
        .map_err(|e| EnricherError::IOError(format!("Could not flush message to enricher: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::child::spawn_child;
    use std::process::Stdio;
    use tokio::{
        process::Command,
        sync::{mpsc::channel, oneshot},
    };

    const MAX_SCHEDULED_JOBS: usize = 32;

    fn fake_child_command(script: &str) -> Command {
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg(script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        command
    }

    #[tokio::test]
    async fn scheduled_worker_command_returns_matching_response() {
        let command = fake_child_command(
            r#"printf '%s\n' '{"messageType":"ready"}'
               while IFS= read -r line; do
                   case "$line" in
                       *'"id":0'*)
                           printf '%s\n' '{"id":0,"context":{"userId":"scheduled-user"}}'
                           ;;
                   esac
               done"#,
        );
        let mut child = spawn_child(1, command)
            .await
            .expect("failed to spawn fake child");
        let (command_tx, mut command_rx) = channel(MAX_SCHEDULED_JOBS);
        let driver = tokio::spawn(async move { driver_loop(1, &mut command_rx, &mut child).await });
        let (respond_to, read_response) = oneshot::channel();

        command_tx
            .send(WorkerCommand::Execute {
                id: 0,
                context: Context::default(),
                headers: HashMap::new(),
                deadline: Instant::now() + Duration::from_secs(1),
                respond_to,
            })
            .await
            .expect("failed to schedule worker command");

        let response = time::timeout(Duration::from_secs(1), read_response)
            .await
            .expect("timed out waiting for scheduled response")
            .expect("scheduled response channel closed")
            .expect("scheduled request failed");

        assert_eq!(response.user_id.as_deref(), Some("scheduled-user"));

        command_tx
            .send(WorkerCommand::Shutdown)
            .await
            .expect("failed to send shutdown command");
        let _ = driver.await.expect("driver task panicked");
    }

    #[tokio::test]
    async fn scheduled_worker_command_times_out_from_driver() {
        let command = fake_child_command(
            r#"printf '%s\n' '{"messageType":"ready"}'
               while IFS= read -r _line; do
                   sleep 1
               done"#,
        );
        let mut child = spawn_child(1, command)
            .await
            .expect("failed to spawn fake child");
        let (command_tx, mut command_rx) = channel(MAX_SCHEDULED_JOBS);
        let driver = tokio::spawn(async move { driver_loop(1, &mut command_rx, &mut child).await });
        let (respond_to, read_response) = oneshot::channel();

        command_tx
            .send(WorkerCommand::Execute {
                id: 0,
                context: Context::default(),
                headers: HashMap::new(),
                deadline: Instant::now() + Duration::from_millis(20),
                respond_to,
            })
            .await
            .expect("failed to schedule worker command");

        let error = time::timeout(Duration::from_secs(1), read_response)
            .await
            .expect("timed out waiting for scheduled timeout")
            .expect("scheduled response channel closed")
            .expect_err("scheduled request should time out");

        match error {
            EnricherError::IOError(message) => assert_eq!(message, "Worker response timed out"),
            other => panic!("unexpected error: {other:?}"),
        }

        command_tx
            .send(WorkerCommand::Shutdown)
            .await
            .expect("failed to send shutdown command");
        let _ = driver.await.expect("driver task panicked");
    }

    #[tokio::test]
    async fn driver_protocol_error_is_returned_to_pending_request() {
        let command = fake_child_command(
            r#"printf '%s\n' '{"messageType":"ready"}'
               IFS= read -r _line
               printf '%s\n' 'not-json'"#,
        );
        let mut child = spawn_child(1, command)
            .await
            .expect("failed to spawn fake child");
        let (command_tx, mut command_rx) = channel(MAX_SCHEDULED_JOBS);
        let driver = tokio::spawn(async move { driver_loop(1, &mut command_rx, &mut child).await });
        let (respond_to, read_response) = oneshot::channel();

        command_tx
            .send(WorkerCommand::Execute {
                id: 0,
                context: Context::default(),
                headers: HashMap::new(),
                deadline: Instant::now() + Duration::from_secs(1),
                respond_to,
            })
            .await
            .expect("failed to schedule worker command");

        let error = time::timeout(Duration::from_secs(1), read_response)
            .await
            .expect("timed out waiting for pending protocol error")
            .expect("pending response channel closed")
            .expect_err("pending request should receive protocol error");

        match error {
            EnricherError::ProtocolError(message) => {
                assert_eq!(message, "child process sent unparsable message: not-json");
            }
            other => panic!("unexpected error: {other:?}"),
        }

        let driver_error = driver
            .await
            .expect("driver task panicked")
            .expect_err("driver should fail on malformed child output");
        match driver_error {
            EnricherError::ProtocolError(message) => {
                assert_eq!(message, "child process sent unparsable message: not-json");
            }
            other => panic!("unexpected driver error: {other:?}"),
        }
    }

    #[tokio::test]
    async fn child_shutdown_is_returned_to_pending_request() {
        let command = fake_child_command(
            r#"printf '%s\n' '{"messageType":"ready"}'
               IFS= read -r _line"#,
        );
        let mut child = spawn_child(1, command)
            .await
            .expect("failed to spawn fake child");
        let (command_tx, mut command_rx) = channel(MAX_SCHEDULED_JOBS);
        let driver = tokio::spawn(async move { driver_loop(1, &mut command_rx, &mut child).await });
        let (respond_to, read_response) = oneshot::channel();

        command_tx
            .send(WorkerCommand::Execute {
                id: 0,
                context: Context::default(),
                headers: HashMap::new(),
                deadline: Instant::now() + Duration::from_secs(1),
                respond_to,
            })
            .await
            .expect("failed to schedule worker command");

        let error = time::timeout(Duration::from_secs(1), read_response)
            .await
            .expect("timed out waiting for child shutdown error")
            .expect("pending response channel closed")
            .expect_err("pending request should receive child shutdown error");

        match error {
            EnricherError::UnexpectedShutdown(message) => {
                assert!(
                    message.contains("child process stdout closed")
                        || message.contains("child process exited with status")
                );
            }
            other => panic!("unexpected error: {other:?}"),
        }

        let _ = driver.await.expect("driver task panicked");
    }

    #[tokio::test]
    async fn shutdown_is_returned_to_pending_request() {
        let command = fake_child_command(
            r#"printf '%s\n' '{"messageType":"ready"}'
               while IFS= read -r _line; do
                   sleep 1
               done"#,
        );
        let mut child = spawn_child(1, command)
            .await
            .expect("failed to spawn fake child");
        let (command_tx, mut command_rx) = channel(MAX_SCHEDULED_JOBS);
        let driver = tokio::spawn(async move { driver_loop(1, &mut command_rx, &mut child).await });
        let (respond_to, read_response) = oneshot::channel();

        command_tx
            .send(WorkerCommand::Execute {
                id: 0,
                context: Context::default(),
                headers: HashMap::new(),
                deadline: Instant::now() + Duration::from_secs(1),
                respond_to,
            })
            .await
            .expect("failed to schedule worker command");
        command_tx
            .send(WorkerCommand::Shutdown)
            .await
            .expect("failed to send shutdown command");

        let error = time::timeout(Duration::from_secs(1), read_response)
            .await
            .expect("timed out waiting for shutdown error")
            .expect("pending response channel closed")
            .expect_err("pending request should receive shutdown error");

        match error {
            EnricherError::IOError(message) => {
                assert_eq!(
                    message,
                    "Worker is shutting down, this job will not be served"
                );
            }
            other => panic!("unexpected error: {other:?}"),
        }

        let _ = driver.await.expect("driver task panicked");
    }

    #[tokio::test]
    #[tracing_test::traced_test]
    async fn late_response_after_timeout_is_logged_as_unknown_request() {
        let command = fake_child_command(
            r#"printf '%s\n' '{"messageType":"ready"}'
               IFS= read -r _line
               sleep 0.1
               printf '%s\n' '{"id":0,"context":{"userId":"late-user"}}'
               while IFS= read -r _line; do
                   sleep 1
               done"#,
        );
        let mut child = spawn_child(1, command)
            .await
            .expect("failed to spawn fake child");
        let (command_tx, mut command_rx) = channel(MAX_SCHEDULED_JOBS);
        let driver = tokio::spawn(async move { driver_loop(1, &mut command_rx, &mut child).await });
        let (respond_to, read_response) = oneshot::channel();

        command_tx
            .send(WorkerCommand::Execute {
                id: 0,
                context: Context::default(),
                headers: HashMap::new(),
                deadline: Instant::now() + Duration::from_millis(20),
                respond_to,
            })
            .await
            .expect("failed to schedule worker command");

        let error = time::timeout(Duration::from_secs(1), read_response)
            .await
            .expect("timed out waiting for scheduled timeout")
            .expect("scheduled response channel closed")
            .expect_err("scheduled request should time out");
        match error {
            EnricherError::IOError(message) => assert_eq!(message, "Worker response timed out"),
            other => panic!("unexpected error: {other:?}"),
        }

        time::timeout(Duration::from_secs(1), async {
            loop {
                if logs_contain("received response for unknown request id 0") {
                    break;
                }

                time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .expect("timed out waiting for late response log");

        command_tx
            .send(WorkerCommand::Shutdown)
            .await
            .expect("failed to send shutdown command");
        let _ = driver.await.expect("driver task panicked");
    }
}
