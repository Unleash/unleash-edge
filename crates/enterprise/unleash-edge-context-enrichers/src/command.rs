use std::fmt::Display;
use tokio::{sync::oneshot::Sender as OneShotSender, time::Instant};
use unleash_types::client_features::Context;

use crate::protocol::{EnrichmentResponse, SerializedEnrichmentRequest};

#[derive(Debug, Clone)]
pub enum EnricherError {
    StartupFailure(String),
    UnexpectedShutdown(String),
    ProtocolError(String),
    ScriptError(String),
    IOError(String),
    Timeout(String),
}

impl Display for EnricherError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EnricherError::StartupFailure(msg) => write!(f, "Startup failure: {msg}"),
            EnricherError::UnexpectedShutdown(msg) => write!(f, "Unexpected shutdown: {msg}"),
            EnricherError::ProtocolError(msg) => write!(f, "Protocol error: {msg}"),
            EnricherError::IOError(msg) => write!(f, "IO error: {msg}"),
            EnricherError::ScriptError(msg) => write!(f, "Script error: {msg}"),
            EnricherError::Timeout(msg) => write!(f, "Timeout error: {msg}"),
        }
    }
}

pub(crate) enum WorkerEvent {
    Response(EnrichmentResponse),
    WorkerError(EnricherError),
}

// Clippy is telling us that we're paying the cost of the large size of the Execute command
// in the Shutdown command as well. Which is fair. But the execution is 99.99% of the commands
// that we will send. We're not optimizing for the shutdown path. Typically the solution here is to
// Box the fields that are causing the large size - Context in this case. But that means
// every request pays the cost of a heap allocation and a pointer indirection. Which is just silly
#[allow(clippy::large_enum_variant)]
pub(crate) enum WorkerCommand {
    Execute {
        id: u64,
        request: SerializedEnrichmentRequest,
        deadline: Instant,
        respond_to: OneShotSender<Result<Context, EnricherError>>,
    },
    Shutdown,
}
