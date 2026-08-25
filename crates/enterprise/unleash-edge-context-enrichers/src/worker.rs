use std::{
    collections::HashMap,
    fmt::Display,
    path::{Path, PathBuf},
    process::Stdio,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines},
    process::{Child, ChildStderr, ChildStdin, ChildStdout, Command},
    sync::{
        mpsc::{Receiver, Sender, channel},
        oneshot::{self, Sender as OneShotSender},
    },
    time::{self, Instant},
};
use tracing::{debug, error, info};
use unleash_types::client_features::Context;

use crate::protocol::{EnrichmentRequest, EnrichmentResponse, ReadyMessage};

const CHILD_MEMORY_CEILING_MB: u64 = 128;
const CHILD_READY_TIMEOUT_IN_SECONDS: u64 = 2;
const MAX_IN_FLIGHT_MESSAGES: usize = 32;
const MAX_SCHEDULED_JOBS: usize = 32;
const PENDING_RESPONSE_EXPIRY_INTERVAL: Duration = Duration::from_millis(10);
// This is the message handling script that executes the messenger protocol on the Node side
// This is absolutely critical for the whole thing to hang together, so relying on a filepath to read this
// feels super fragile. Luckily, we don't have to do that - we can just bake the whole thing
// in the Edge binary itself and then feed it to the Node process on startup
const WORKER_SCRIPT: &str = include_str!("../worker_script.js");

#[derive(Debug, Clone)]
pub enum EnricherError {
    StartupFailure(String),
    UnexpectedShutdown(String),
    ProtocolError(String),
    ScriptError(String),
    IOError(String),
}

impl Display for EnricherError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EnricherError::StartupFailure(msg) => write!(f, "Startup failure: {msg}"),
            EnricherError::UnexpectedShutdown(msg) => write!(f, "Unexpected shutdown: {msg}"),
            EnricherError::ProtocolError(msg) => write!(f, "Protocol error: {msg}"),
            EnricherError::IOError(msg) => write!(f, "IO error: {msg}"),
            EnricherError::ScriptError(msg) => write!(f, "Script error: {msg}"),
        }
    }
}

enum WorkerEvent {
    Response(EnrichmentResponse),
    WorkerError(String),
}

struct RunningNodeChild {
    child: Child,
    pid: u32,
    child_input: ChildStdin,
    child_output: Receiver<WorkerEvent>,
}

impl RunningNodeChild {
    async fn terminate(&mut self) -> Result<(), EnricherError> {
        self.child.kill().await.map_err(|e| {
            EnricherError::UnexpectedShutdown(format!("Failed to terminate child process: {}", e))
        })
    }
}

pub struct NodeWorkerController {
    #[expect(dead_code)]
    worker_id: u32,
    next_request_id: AtomicU64,
    command_tx: Sender<WorkerCommand>,
}

// Clippy is telling us that we're paying the cost of the large size of the Execute command
// in the Shutdown command as well. Which is fair. But the execution is 99.99% of the commands
// that we will send. We're not optimizing for the shutdown path. Typically the solution here is to
// Box the fields that are causing the large size - Context in this case. But that means
// every request pays the cost of a heap allocation and a pointer indirection. Which is just silly
#[allow(clippy::large_enum_variant)]
enum WorkerCommand {
    Execute {
        id: u64,
        context: Context,
        headers: HashMap<String, String>,
        deadline: Instant,
        respond_to: OneShotSender<Result<Context, EnricherError>>,
    },
    Shutdown,
}

struct PendingResponse {
    deadline: Instant,
    respond_to: OneShotSender<Result<Context, EnricherError>>,
}

impl NodeWorkerController {
    #[expect(dead_code)]
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

    #[cfg_attr(not(test), expect(dead_code))]
    pub async fn request_enrichment(
        &self,
        context: Context,
        headers: HashMap<String, String>,
        job_timeout: Duration,
    ) -> Result<Context, EnricherError> {
        let (respond_to, read_response) = oneshot::channel();
        let deadline = Instant::now() + job_timeout;

        let command = WorkerCommand::Execute {
            id: self.next_request_id.fetch_add(1, Ordering::SeqCst),
            context,
            headers,
            deadline,
            respond_to,
        };

        time::timeout_at(deadline, self.command_tx.send(command))
            .await
            .map_err(|_| EnricherError::IOError("Worker response timed out".to_string()))?
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

async fn spawn_node_child_process(
    worker_id: u32,
    enricher_script: &Path,
) -> Result<RunningNodeChild, EnricherError> {
    spawn_child(worker_id, node_worker_command(enricher_script)).await
}

fn node_worker_command(enricher_script: &Path) -> Command {
    let mut command = Command::new(resolve_node_path());
    command
        .arg(format!("--max-old-space-size={}", CHILD_MEMORY_CEILING_MB))
        .arg("--eval")
        .arg(WORKER_SCRIPT)
        .arg("--")
        .arg("--enricher-script")
        .arg(enricher_script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_clear()
        .kill_on_drop(true);
    command
}

async fn spawn_child(
    worker_id: u32,
    mut command: Command,
) -> Result<RunningNodeChild, EnricherError> {
    let (event_tx, event_rx) = channel(MAX_IN_FLIGHT_MESSAGES);

    let mut child = command.spawn().map_err(|e| {
        EnricherError::StartupFailure(format!("Failed to spawn child process: {}", e))
    })?;
    let child_pid = child
        .id()
        // tokio docs say this doesn't happen unless the process immediately terminates. We very much
        // do not expect that to happen because the Node process should be running an infinite
        // receiver loop so this is Bad™ and someone needs to know about it
        .ok_or_else(|| EnricherError::StartupFailure("Failed to get child PID".to_string()))?;

    let std_in = child
        .stdin
        .take()
        .ok_or_else(|| EnricherError::StartupFailure("Failed to open child stdin".to_string()))?;

    let std_out = child
        .stdout
        .take()
        .ok_or_else(|| EnricherError::StartupFailure("Failed to open child stdout".to_string()))?;

    drain_error_stream(worker_id, child_pid, child.stderr.take());

    let mut lines = BufReader::new(std_out).lines();
    wait_for_ready(&mut lines).await?;

    tokio::spawn(read_child_messages(worker_id, child_pid, lines, event_tx));

    Ok(RunningNodeChild {
        child,
        pid: child_pid,
        child_input: std_in,
        child_output: event_rx,
    })
}

async fn wait_for_ready(lines: &mut Lines<BufReader<ChildStdout>>) -> Result<(), EnricherError> {
    let line = time::timeout(
        Duration::from_secs(CHILD_READY_TIMEOUT_IN_SECONDS),
        lines.next_line(),
    )
    .await
    .map_err(|_| EnricherError::StartupFailure("worker readiness timed out".into()))?
    .map_err(|e| EnricherError::StartupFailure(format!("worker readiness read failed: {e}")))?;

    let line = line.ok_or(EnricherError::StartupFailure(
        "worker failed to report readiness state".into(),
    ))?;

    let ready = serde_json::from_str::<ReadyMessage>(&line).map_err(|_| {
        EnricherError::StartupFailure("worker readiness failed: unparsable response".into())
    })?;

    if ready._message_type != "ready" {
        return Err(EnricherError::StartupFailure(format!(
            "worker readiness failed: unexpected messageType '{}'",
            ready._message_type
        )));
    }
    // Message is just marker so we don't really care about it, only that it was received
    // so we chuck it in the bin and let the caller know we're good to go
    Ok(())
}

async fn read_child_messages(
    worker_id: u32,
    child_pid: u32,
    mut child_std_out: Lines<BufReader<ChildStdout>>,
    event_tx: Sender<WorkerEvent>,
) {
    loop {
        match child_std_out.next_line().await {
            Ok(Some(line)) => {
                let message_result =
                    if let Ok(event) = serde_json::from_str::<EnrichmentResponse>(&line) {
                        event_tx.send(WorkerEvent::Response(event)).await
                    } else {
                        event_tx.send(WorkerEvent::WorkerError(line)).await
                    };

                // this happens if the child manages to flush one last message to its stdout but the Rust side receiver
                // has already been dropped. This should basically not happen if everything else is working right
                // but it's also not harmful if it does
                if message_result.is_err() {
                    debug!(
                        "[node-worker {worker_id} pid={child_pid}] event receiver closed; stopping stdout reader"
                    );
                    break;
                }
            }
            Err(error) => {
                let _ = event_tx
                    .send(WorkerEvent::WorkerError(format!(
                        "child stdout read failed: {error}"
                    )))
                    .await;
                break;
            }
            Ok(None) => break,
        }
    }
}

async fn driver_loop(
    worker_id: u32,
    command_rx: &mut Receiver<WorkerCommand>,
    child: &mut RunningNodeChild,
) -> Result<(), EnricherError> {
    let mut pending_responses = HashMap::new();
    let mut pending_expiry = time::interval(PENDING_RESPONSE_EXPIRY_INTERVAL);

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

// The intent is to pipe user script's std out and std err to the process std err. That way when someone leaves a bunch
// of console logs in the script, it doesn't break our IPC protocol. So this is just a polite way of making sure
// that stream doesn't fill its own buffers and block while still giving the user a way to see what their script is doing.
// We also really don't care about this from our system's perspective so it gets a lazy background task to just OMNOMNOMNOM the stream
fn drain_error_stream(worker_id: u32, child_pid: u32, stderr: Option<ChildStderr>) {
    let Some(stderr) = stderr else {
        return;
    };
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        loop {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    info!("[node-worker {worker_id} pid={child_pid}] {line}");
                }
                Err(error) => {
                    error!(
                        "[node-worker {worker_id} pid={child_pid}] stopped draining stderr after read failure: {error}"
                    );
                    break;
                }
                Ok(None) => break,
            }
        }
    });
}

// spawn_node_child_process is wiping the environment so we need to resolve the path to node ourselves
// because now only the parent is able to see the configured env vars. Bit of a hack. Three paths to deal with this
// 1) Throw all this in the bin and compose a JS runtime from parts - more work, out of scope for an MVP
// 2) Don't wipe the environment before spawning the child, which gives the child more scope to do bad things
// 3) Leave it alone - probably fine for an MVP middle ground
fn resolve_node_path() -> PathBuf {
    let node = PathBuf::from("node");
    let Some(path) = std::env::var_os("PATH") else {
        return node;
    };

    std::env::split_paths(&path)
        .map(|path| path.join("node"))
        .find(|path| path.is_file())
        .unwrap_or(node)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, process::Command as StdCommand};
    use tempfile::{NamedTempFile, TempPath};
    use tokio::io::AsyncWriteExt;

    fn node_is_available() -> bool {
        StdCommand::new(resolve_node_path())
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
    }

    fn write_temp_enricher(source: &str) -> TempPath {
        let file = NamedTempFile::with_suffix(".cjs").expect("failed to create temp enricher file");
        let path = file.into_temp_path();
        fs::write(&path, source).expect("failed to write temp enricher script");
        path
    }

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
    async fn worker_sends_and_reads_happy_path_messages_to_child() {
        let command = fake_child_command(
            r#"printf '%s\n' '{"messageType":"ready"}'
               while IFS= read -r _line; do
                   printf '%s\n' '{"id":42,"context":{"userId":"fake-user"}}'
               done"#,
        );

        let mut child = spawn_child(1, command)
            .await
            .expect("failed to spawn fake child");

        child
            .child_input
            .write_all(br#"{"id":42,"context":{}}"#)
            .await
            .expect("failed to write fake request");
        child
            .child_input
            .write_all(b"\n")
            .await
            .expect("failed to terminate fake request");

        let event = time::timeout(Duration::from_secs(1), child.child_output.recv())
            .await
            .expect("timed out waiting for fake child event")
            .expect("fake child event stream closed");

        match event {
            WorkerEvent::Response(response) => {
                assert_eq!(response.id, 42);
                assert_eq!(
                    response.outcome.unwrap().user_id.as_deref(),
                    Some("fake-user")
                );
            }
            WorkerEvent::WorkerError(error) => {
                panic!("unexpected fake child event: {error}");
            }
        }
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
    async fn request_enrichment_times_out_when_scheduler_queue_is_full() {
        let (command_tx, _command_rx) = channel(1);
        command_tx
            .send(WorkerCommand::Shutdown)
            .await
            .expect("failed to fill scheduler queue");
        let worker = NodeWorkerController {
            worker_id: 1,
            next_request_id: AtomicU64::new(0),
            command_tx,
        };

        let error = worker
            .request_enrichment(
                Context::default(),
                HashMap::new(),
                Duration::from_millis(20),
            )
            .await
            .expect_err("request should time out waiting for scheduler queue capacity");

        match error {
            EnricherError::IOError(message) => assert_eq!(message, "Worker response timed out"),
            other => panic!("unexpected error: {other:?}"),
        }
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

    #[tokio::test]
    async fn protocol_errors_from_child_are_received_in_parent() {
        let command = fake_child_command(
            r#"printf '%s\n' '{"messageType":"ready"}'
               printf '%s\n' 'not-json'"#,
        );

        let mut child = spawn_child(1, command)
            .await
            .expect("failed to spawn fake child");

        let event = time::timeout(Duration::from_secs(1), child.child_output.recv())
            .await
            .expect("timed out waiting for fake child event")
            .expect("fake child event stream closed");

        match event {
            WorkerEvent::WorkerError(line) => assert_eq!(line, "not-json"),
            WorkerEvent::Response(_) => panic!("unexpected enrichment response"),
        }
    }

    #[tokio::test]
    async fn worker_happy_path_messages_work_end_to_end() {
        if !node_is_available() {
            return;
        }

        let enricher_script = write_temp_enricher(
            r#"
            module.exports = async (context) => ({
                ...context,
                userId: "real-worker-user",
            });
            "#,
        );
        let mut child = spawn_node_child_process(1, &enricher_script)
            .await
            .expect("failed to spawn node child process");

        child
            .child_input
            .write_all(br#"{"id":9,"context":{"userId":"original-user"}}"#)
            .await
            .expect("failed to write enrichment request");
        child
            .child_input
            .write_all(b"\n")
            .await
            .expect("failed to terminate enrichment request");

        let event = time::timeout(Duration::from_secs(1), child.child_output.recv())
            .await
            .expect("timed out waiting for worker event")
            .expect("worker event stream closed");

        match event {
            WorkerEvent::Response(response) => {
                assert_eq!(response.id, 9);
                assert_eq!(
                    response.outcome.unwrap().user_id.as_deref(),
                    Some("real-worker-user")
                );
            }
            WorkerEvent::WorkerError(error) => {
                panic!("unexpected worker event: {error}");
            }
        }

        let _ = child.child.start_kill();
    }

    #[tokio::test]
    #[tracing_test::traced_test]
    async fn worker_traps_console_messages_and_pipes_them_to_logs() {
        let mut child = spawn_child(
            1,
            fake_child_command(
                r#"printf '%s\n' '{"messageType":"ready"}'
                   printf '%s\n' '[console.log] hello from enricher {"ok":true}' >&2
                   while IFS= read -r _line; do
                       printf '%s\n' '{"id":8,"error":"script blew up"}'
                   done"#,
            ),
        )
        .await
        .expect("failed to spawn fake child");

        child
            .child_input
            .write_all(br#"{"id":8,"context":{}}"#)
            .await
            .expect("failed to write fake request");
        child
            .child_input
            .write_all(b"\n")
            .await
            .expect("failed to terminate fake request");

        let event = time::timeout(Duration::from_secs(1), child.child_output.recv())
            .await
            .expect("timed out waiting for fake child event")
            .expect("fake child event stream closed");

        match event {
            WorkerEvent::Response(response) => {
                assert_eq!(response.id, 8);
                assert_eq!(response.outcome.unwrap_err(), "script blew up");
            }
            WorkerEvent::WorkerError(error) => {
                panic!("unexpected fake child event: {error}");
            }
        }

        time::timeout(Duration::from_secs(1), async {
            loop {
                if logs_contain("[node-worker 1 pid=")
                    && logs_contain("[console.log] hello from enricher {\"ok\":true}")
                {
                    break;
                }

                time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .expect("timed out waiting for drained stderr log");

        let _ = child.child.start_kill();
    }
}
