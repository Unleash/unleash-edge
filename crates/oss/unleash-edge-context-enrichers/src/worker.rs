use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicU32, AtomicU64, Ordering},
    },
};

use thiserror::Error;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines},
    process::{Child, ChildStdin, ChildStdout, Command},
    sync::{mpsc, oneshot},
    time::{self, Duration},
};

use crate::protocol::{EnrichmentRequest, EnrichmentResponse, ReadyMessage};

#[derive(Debug, Error)]
pub enum EnrichmentError {
    #[error("enrichment timed out")]
    Timeout,

    #[error("customer script failed: {0}")]
    Script(String),

    #[error("node worker exited")]
    WorkerExited,

    #[error("node worker was restarted")]
    WorkerRestarted,

    #[error("node worker is unavailable: {0}")]
    WorkerUnavailable(String),

    #[error("protocol error: {0}")]
    Protocol(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Clone)]
pub struct NodeWorker {
    worker_id: usize,
    command_tx: mpsc::Sender<WorkerCommand>,
    next_request_id: Arc<AtomicU64>,
    pid: Arc<AtomicU32>,
}

struct RunningChild {
    child: Child,
    stdin: ChildStdin,
    event_rx: mpsc::Receiver<WorkerEvent>,
}

enum WorkerCommand {
    Execute {
        id: u64,
        context: serde_json::Value,
        respond_to: oneshot::Sender<Result<serde_json::Value, EnrichmentError>>,
    },
    Restart {
        reason: String,
    },
    Shutdown,
}

enum WorkerEvent {
    Response(Result<EnrichmentResponse, EnrichmentError>),
    StdoutClosed,
}

impl NodeWorker {
    pub async fn start(
        worker_id: usize,
        worker_script: PathBuf,
        customer_script: PathBuf,
    ) -> Result<Self, EnrichmentError> {
        let (command_tx, command_rx) = mpsc::channel(64);
        let next_request_id = Arc::new(AtomicU64::new(1));
        let pid = Arc::new(AtomicU32::new(0));
        let node_executable = resolve_node_executable();

        let running = spawn_child(
            worker_id,
            &node_executable,
            &worker_script,
            &customer_script,
        )
        .await?;
        pid.store(running.child.id().unwrap_or(0), Ordering::SeqCst);

        tokio::spawn(driver_loop(
            worker_id,
            node_executable,
            worker_script,
            customer_script,
            command_rx,
            pid.clone(),
            running,
        ));

        Ok(Self {
            worker_id,
            command_tx,
            next_request_id,
            pid,
        })
    }

    pub async fn execute(
        &self,
        context: serde_json::Value,
        timeout: Duration,
    ) -> Result<serde_json::Value, EnrichmentError> {
        let id = self.next_request_id.fetch_add(1, Ordering::SeqCst);
        let (respond_to, response_rx) = oneshot::channel();

        self.command_tx
            .send(WorkerCommand::Execute {
                id,
                context,
                respond_to,
            })
            .await
            .map_err(|_| EnrichmentError::WorkerUnavailable("worker driver stopped".into()))?;

        match time::timeout(timeout, response_rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(EnrichmentError::WorkerUnavailable(
                "worker dropped response channel".into(),
            )),
            Err(_) => {
                let _ = self
                    .command_tx
                    .send(WorkerCommand::Restart {
                        reason: format!("request {id} timed out after {timeout:?}"),
                    })
                    .await;
                Err(EnrichmentError::Timeout)
            }
        }
    }

    pub fn pid(&self) -> u32 {
        self.pid.load(Ordering::SeqCst)
    }

    pub async fn shutdown(&self) {
        let _ = self.command_tx.send(WorkerCommand::Shutdown).await;
    }
}

impl std::fmt::Debug for NodeWorker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NodeWorker")
            .field("worker_id", &self.worker_id)
            .field("pid", &self.pid())
            .finish_non_exhaustive()
    }
}

pub fn default_worker_script_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts/worker.js")
}

async fn driver_loop(
    worker_id: usize,
    node_executable: PathBuf,
    worker_script: PathBuf,
    customer_script: PathBuf,
    mut command_rx: mpsc::Receiver<WorkerCommand>,
    pid: Arc<AtomicU32>,
    mut running: RunningChild,
) {
    let mut pending = HashMap::new();

    loop {
        tokio::select! {
            command = command_rx.recv() => {
                match command {
                    Some(WorkerCommand::Execute { id, context, respond_to }) => {
                        if let Err(error) = write_request(&mut running.stdin, id, context).await {
                            let _ = respond_to.send(Err(error));
                            restart_after_failure(
                                worker_id,
                                &node_executable,
                                &worker_script,
                                &customer_script,
                                &pid,
                                &mut running,
                                &mut pending,
                                "failed to write request",
                            ).await;
                        } else {
                            pending.insert(id, respond_to);
                        }
                    }
                    Some(WorkerCommand::Restart { reason }) => {
                        eprintln!("[node-worker {worker_id} pid={}] restarting: {reason}", pid.load(Ordering::SeqCst));
                        restart_after_failure(
                            worker_id,
                            &node_executable,
                            &worker_script,
                            &customer_script,
                            &pid,
                            &mut running,
                            &mut pending,
                            &reason,
                        ).await;
                    }
                    Some(WorkerCommand::Shutdown) | None => {
                        terminate_child(&mut running.child).await;
                        fail_pending(&mut pending, EnrichmentError::WorkerRestarted);
                        break;
                    }
                }
            }
            event = running.event_rx.recv() => {
                match event {
                    Some(WorkerEvent::Response(Ok(response))) => {
                        resolve_response(worker_id, &mut pending, response);
                    }
                    Some(WorkerEvent::Response(Err(error))) => {
                        eprintln!("[node-worker {worker_id} pid={}] {error}", pid.load(Ordering::SeqCst));
                    }
                    Some(WorkerEvent::StdoutClosed) | None => {
                        restart_after_failure(
                            worker_id,
                            &node_executable,
                            &worker_script,
                            &customer_script,
                            &pid,
                            &mut running,
                            &mut pending,
                            "stdout closed",
                        ).await;
                    }
                }
            }
            status = running.child.wait() => {
                let reason = match status {
                    Ok(status) => format!("worker exited with {status}"),
                    Err(error) => format!("failed to wait for worker exit: {error}"),
                };
                restart_after_failure(
                    worker_id,
                    &node_executable,
                    &worker_script,
                    &customer_script,
                    &pid,
                    &mut running,
                    &mut pending,
                    &reason,
                ).await;
            }
        }
    }
}

async fn write_request(
    stdin: &mut ChildStdin,
    id: u64,
    context: serde_json::Value,
) -> Result<(), EnrichmentError> {
    let request = EnrichmentRequest { id, context };
    let mut line = serde_json::to_vec(&request)?;
    line.push(b'\n');
    stdin.write_all(&line).await?;
    stdin.flush().await?;
    Ok(())
}

fn resolve_response(
    worker_id: usize,
    pending: &mut HashMap<u64, oneshot::Sender<Result<serde_json::Value, EnrichmentError>>>,
    response: EnrichmentResponse,
) {
    let Some(respond_to) = pending.remove(&response.id) else {
        eprintln!(
            "[node-worker {worker_id}] ignored response for unknown request {}",
            response.id
        );
        return;
    };

    let result = match (response.result, response.error) {
        (Some(result), None) => Ok(result),
        (None, Some(error)) => Err(EnrichmentError::Script(error)),
        (result, error) => Err(EnrichmentError::Protocol(format!(
            "expected exactly one of result or error, got result={} error={}",
            result.is_some(),
            error.is_some()
        ))),
    };

    let _ = respond_to.send(result);
}

async fn restart_after_failure(
    worker_id: usize,
    node_executable: &Path,
    worker_script: &Path,
    customer_script: &Path,
    pid: &AtomicU32,
    running: &mut RunningChild,
    pending: &mut HashMap<u64, oneshot::Sender<Result<serde_json::Value, EnrichmentError>>>,
    reason: &str,
) {
    terminate_child(&mut running.child).await;
    fail_pending(pending, EnrichmentError::WorkerRestarted);

    match spawn_child(worker_id, node_executable, worker_script, customer_script).await {
        Ok(replacement) => {
            let new_pid = replacement.child.id().unwrap_or(0);
            eprintln!("[node-worker {worker_id} pid={new_pid}] ready after restart: {reason}");
            pid.store(new_pid, Ordering::SeqCst);
            *running = replacement;
        }
        Err(error) => {
            eprintln!("[node-worker {worker_id}] replacement failed: {error}");
            pid.store(0, Ordering::SeqCst);
        }
    }
}

fn fail_pending(
    pending: &mut HashMap<u64, oneshot::Sender<Result<serde_json::Value, EnrichmentError>>>,
    error: EnrichmentError,
) {
    for (_, respond_to) in pending.drain() {
        let _ = respond_to.send(Err(match &error {
            EnrichmentError::Timeout => EnrichmentError::Timeout,
            EnrichmentError::WorkerExited => EnrichmentError::WorkerExited,
            EnrichmentError::WorkerRestarted => EnrichmentError::WorkerRestarted,
            EnrichmentError::WorkerUnavailable(message) => {
                EnrichmentError::WorkerUnavailable(message.clone())
            }
            EnrichmentError::Script(message) => EnrichmentError::Script(message.clone()),
            EnrichmentError::Protocol(message) => EnrichmentError::Protocol(message.clone()),
            EnrichmentError::Io(err) => EnrichmentError::WorkerUnavailable(err.to_string()),
            EnrichmentError::Json(err) => EnrichmentError::Protocol(err.to_string()),
        }));
    }
}

async fn terminate_child(child: &mut Child) {
    let _ = child.start_kill();
    let _ = child.wait().await;
}

async fn spawn_child(
    worker_id: usize,
    node_executable: &Path,
    worker_script: &Path,
    customer_script: &Path,
) -> Result<RunningChild, EnrichmentError> {
    let mut command = Command::new(node_executable);
    command
        .arg("--max-old-space-size=128")
        .arg(worker_script)
        .arg(customer_script)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = command.spawn()?;
    let child_pid = child.id().unwrap_or(0);
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| EnrichmentError::WorkerUnavailable("missing child stdin".into()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| EnrichmentError::WorkerUnavailable("missing child stdout".into()))?;

    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            loop {
                match lines.next_line().await {
                    Ok(Some(line)) => {
                        eprintln!("[node-worker {worker_id} pid={child_pid}] {line}");
                    }
                    Ok(None) => break,
                    Err(error) => {
                        eprintln!(
                            "[node-worker {worker_id} pid={child_pid}] stderr read failed: {error}"
                        );
                        break;
                    }
                }
            }
        });
    }

    let mut lines = BufReader::new(stdout).lines();
    wait_for_ready(worker_id, child_pid, &mut lines).await?;

    let (event_tx, event_rx) = mpsc::channel(64);
    tokio::spawn(read_stdout(worker_id, child_pid, lines, event_tx));

    eprintln!("[node-worker {worker_id} pid={child_pid}] ready");

    Ok(RunningChild {
        child,
        stdin,
        event_rx,
    })
}

fn resolve_node_executable() -> PathBuf {
    let node = PathBuf::from("node");
    let Some(path) = std::env::var_os("PATH") else {
        return node;
    };

    std::env::split_paths(&path)
        .map(|path| path.join("node"))
        .find(|path| path.is_file())
        .unwrap_or(node)
}

async fn wait_for_ready(
    worker_id: usize,
    child_pid: u32,
    lines: &mut Lines<BufReader<ChildStdout>>,
) -> Result<(), EnrichmentError> {
    let line = time::timeout(Duration::from_secs(2), lines.next_line())
        .await
        .map_err(|_| EnrichmentError::WorkerUnavailable("worker readiness timed out".into()))??;
    let line = line.ok_or(EnrichmentError::WorkerExited)?;
    let ready: ReadyMessage = serde_json::from_str(&line)?;

    if ready.message_type == "ready" {
        Ok(())
    } else {
        Err(EnrichmentError::Protocol(format!(
            "worker {worker_id} pid {child_pid} sent unexpected startup message: {line}"
        )))
    }
}

async fn read_stdout(
    worker_id: usize,
    child_pid: u32,
    mut lines: Lines<BufReader<ChildStdout>>,
    event_tx: mpsc::Sender<WorkerEvent>,
) {
    loop {
        match lines.next_line().await {
            Ok(Some(line)) => {
                let event = match serde_json::from_str::<EnrichmentResponse>(&line) {
                    Ok(response) => WorkerEvent::Response(Ok(response)),
                    Err(error) => WorkerEvent::Response(Err(EnrichmentError::Json(error))),
                };

                if event_tx.send(event).await.is_err() {
                    break;
                }
            }
            Ok(None) => {
                let _ = event_tx.send(WorkerEvent::StdoutClosed).await;
                break;
            }
            Err(error) => {
                let _ = event_tx
                    .send(WorkerEvent::Response(Err(EnrichmentError::Io(error))))
                    .await;
                break;
            }
        }
    }

    eprintln!("[node-worker {worker_id} pid={child_pid}] stdout reader stopped");
}
