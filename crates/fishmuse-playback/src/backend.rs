use async_trait::async_trait;
use fishmuse_domain::{AppResult, TrackId};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::{PlaybackCommand, PlaybackStatus};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackBackendKind {
    Foobar2000,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PlaybackSnapshot {
    pub revision: u64,
    pub status: PlaybackStatus,
    pub track_id: Option<TrackId>,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    pub volume: f32,
    pub backend: PlaybackBackendKind,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "event", content = "payload", rename_all = "snake_case")]
pub enum PlaybackEvent {
    Snapshot(PlaybackSnapshot),
    Disconnected {
        revision: u64,
        backend: PlaybackBackendKind,
    },
}

impl PlaybackEvent {
    #[must_use]
    pub const fn revision(&self) -> u64 {
        match self {
            Self::Snapshot(snapshot) => snapshot.revision,
            Self::Disconnected { revision, .. } => *revision,
        }
    }

    #[must_use]
    pub fn into_snapshot(self) -> PlaybackSnapshot {
        match self {
            Self::Snapshot(snapshot) => snapshot,
            Self::Disconnected { revision, backend } => PlaybackSnapshot {
                revision,
                status: PlaybackStatus::Unavailable,
                track_id: None,
                position_ms: 0,
                duration_ms: None,
                volume: 1.0,
                backend,
            },
        }
    }
}

#[async_trait]
pub trait PlaybackBackend: Send + Sync {
    async fn execute(&self, command: PlaybackCommand) -> AppResult<PlaybackSnapshot>;
    async fn snapshot(&self) -> AppResult<PlaybackSnapshot>;
    fn subscribe(&self) -> broadcast::Receiver<PlaybackEvent>;
}
