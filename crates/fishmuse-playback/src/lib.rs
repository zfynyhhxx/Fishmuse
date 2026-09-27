#![forbid(unsafe_code)]

mod backend;
mod command;
mod listening;
mod manager;
mod operation_store;
mod state;

pub use backend::{PlaybackBackend, PlaybackBackendKind, PlaybackEvent, PlaybackSnapshot};
pub use command::{CommandFingerprint, PlaybackCommand};
pub use listening::{Clock, ListenTracker, ListeningEvent, ListeningSink, SystemClock};
pub use manager::PlaybackManager;
pub use operation_store::{MemoryOperationStore, OperationClaim, OperationStore};
pub use state::{PlaybackStateMachine, PlaybackStatus};
