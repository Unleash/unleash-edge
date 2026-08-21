use std::{path::PathBuf, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, BufReader, Lines},
    process::{Child, ChildStderr, ChildStdin, ChildStdout, Command},
    sync::mpsc::{Receiver, Sender},
    time,
};
use tracing::{debug, error, info};

use crate::protocol::{EnrichmentResponse, ReadyMessage};

const CHILD_MEMORY_CEILING_MB: u64 = 128;
const CHILD_READY_TIMEOUT_IN_SECONDS: u64 = 2;
const MAX_IN_FLIGHT_MESSAGES: usize = 32;
// This is the message handling script that executes the messenger protocol on the Node side
// This is absolutely critical for the whole thing to hang together, so relying on a filepath to read this
// feels super fragile. Luckily, we don't have to do that - we can just bake the whole thing
// in the Edge binary itself and then feed it to the Node process on startup
const WORKER_SCRIPT: &str = include_str!("../worker_script.js");

#[derive(Debug)]
#[expect(dead_code)]
pub enum EnricherError {
    StartupFailure(String),
    UnexpectedShutdown(String),
    ProtocolError(String),
    IOError(String),
}

#[expect(dead_code)]
enum WorkerEvent {
    Response(EnrichmentResponse),
    BrokenPipe(String),
    ProtocolError(String),
}

#[expect(dead_code)]
struct RunningNodeChild {
    child: Child,
    child_input: ChildStdin,
    child_output: Receiver<WorkerEvent>,
}

#[expect(dead_code)]
pub struct NodeWorkerController {
    worker_id: u32,
    child: RunningNodeChild,
}

impl NodeWorkerController {
    #[expect(dead_code)]
    pub async fn start(worker_id: u32) -> Result<Self, EnricherError> {
        let child = spawn_node_child_process(worker_id).await?;

        Ok(NodeWorkerController { worker_id, child })
    }
}

async fn spawn_node_child_process(worker_id: u32) -> Result<RunningNodeChild, EnricherError> {
    let mut command = Command::new(resolve_node_path());
    let (event_tx, event_rx) = tokio::sync::mpsc::channel(MAX_IN_FLIGHT_MESSAGES);

    command
        .arg(format!("--max-old-space-size={}", CHILD_MEMORY_CEILING_MB))
        .arg("--eval")
        .arg(WORKER_SCRIPT)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_clear()
        .kill_on_drop(true);

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
    .map_err(|_| EnricherError::StartupFailure("worker readiness read failed".into()))?;

    let line = line.ok_or(EnricherError::StartupFailure(
        "worker failed to report readiness state".into(),
    ))?;

    serde_json::from_str::<ReadyMessage>(&line).map_err(|_| {
        EnricherError::StartupFailure("worker readiness failed: unparsable reponse".into())
    })?;

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
                        event_tx.send(WorkerEvent::ProtocolError(line)).await
                    };

                // this happens if the child manages to flush one last message to it's std out but the Rust side receiver
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
                    .send(WorkerEvent::BrokenPipe(format!(
                        "child stdout read failed: {error}"
                    )))
                    .await;
                break;
            }
            Ok(None) => break,
        }
    }
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
