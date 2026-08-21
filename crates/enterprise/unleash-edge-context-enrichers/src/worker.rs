use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
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

#[cfg_attr(not(test), expect(dead_code))]
enum WorkerEvent {
    Response(EnrichmentResponse),
    BrokenPipe(String),
    ProtocolError(String),
}

#[cfg_attr(not(test), expect(dead_code))]
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
    pub async fn start(worker_id: u32, enricher_script: &Path) -> Result<Self, EnricherError> {
        let child = spawn_node_child_process(worker_id, enricher_script).await?;

        Ok(NodeWorkerController { worker_id, child })
    }
}

async fn spawn_node_child_process(
    worker_id: u32,
    enricher_script: &Path,
) -> Result<RunningNodeChild, EnricherError> {
    spawn_configured_child(worker_id, node_worker_command(enricher_script)).await
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

async fn spawn_configured_child(
    worker_id: u32,
    mut command: Command,
) -> Result<RunningNodeChild, EnricherError> {
    let (event_tx, event_rx) = tokio::sync::mpsc::channel(MAX_IN_FLIGHT_MESSAGES);

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
                        event_tx.send(WorkerEvent::ProtocolError(line)).await
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, process::Command as StdCommand};
    use tempfile::{NamedTempFile, TempPath};
    use tokio::io::AsyncWriteExt;

    fn node_is_available() -> bool {
        // very stupid hack - right now CI doesn't have a Node runtime
        // available and I don't want to add it. So we'll skip these on CI
        // but I'm a banana who forgets things so I'm putting a time-bomb on this
        // if this check is still here Aug 26 we fail so someone (me) needs to fix it

        assert!(
            std::time::SystemTime::now()
                < std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1787695200)
        );

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

        let mut child = spawn_configured_child(1, command)
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
            WorkerEvent::BrokenPipe(error) | WorkerEvent::ProtocolError(error) => {
                panic!("unexpected fake child event: {error}");
            }
        }
    }

    #[tokio::test]
    async fn protocol_errors_from_child_are_received_in_parent() {
        let command = fake_child_command(
            r#"printf '%s\n' '{"messageType":"ready"}'
               printf '%s\n' 'not-json'"#,
        );

        let mut child = spawn_configured_child(1, command)
            .await
            .expect("failed to spawn fake child");

        let event = time::timeout(Duration::from_secs(1), child.child_output.recv())
            .await
            .expect("timed out waiting for fake child event")
            .expect("fake child event stream closed");

        match event {
            WorkerEvent::ProtocolError(line) => assert_eq!(line, "not-json"),
            WorkerEvent::Response(_) => panic!("unexpected enrichment response"),
            WorkerEvent::BrokenPipe(error) => panic!("unexpected broken pipe: {error}"),
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
            WorkerEvent::BrokenPipe(error) | WorkerEvent::ProtocolError(error) => {
                panic!("unexpected worker event: {error}");
            }
        }

        let _ = child.child.start_kill();
    }

    #[tokio::test]
    #[tracing_test::traced_test]
    async fn worker_traps_console_messages_and_pipes_them_to_logs() {
        let mut child = spawn_configured_child(
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
            WorkerEvent::BrokenPipe(error) | WorkerEvent::ProtocolError(error) => {
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
