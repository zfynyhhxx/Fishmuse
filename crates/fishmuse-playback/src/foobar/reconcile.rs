use fishmuse_domain::AppResult;
use uuid::Uuid;

use crate::{PlaybackSnapshot, state::playback_error};

use super::protocol::StateSnapshot;

#[derive(Debug, Default)]
pub struct StateReconciler {
    session_id: Option<Uuid>,
    remote_sequence: Option<u64>,
    application_revision: Option<u64>,
}

impl StateReconciler {
    pub fn begin_session(&mut self, snapshot: StateSnapshot) -> AppResult<PlaybackSnapshot> {
        let next_revision = match self.application_revision {
            Some(revision) => revision.saturating_add(1),
            None => snapshot.revision,
        };
        let session_id = snapshot.session_id;
        let remote_revision = snapshot.revision;
        let playback = to_playback_snapshot(snapshot, next_revision)?;
        self.session_id = Some(session_id);
        self.remote_sequence = Some(remote_revision);
        self.application_revision = Some(next_revision);
        Ok(playback)
    }

    pub fn apply_event(&mut self, snapshot: StateSnapshot) -> AppResult<Option<PlaybackSnapshot>> {
        if self.session_id != Some(snapshot.session_id)
            || self
                .remote_sequence
                .is_some_and(|sequence| snapshot.revision <= sequence)
        {
            return Ok(None);
        }

        let next_revision = self
            .application_revision
            .unwrap_or_default()
            .saturating_add(1);
        let remote_revision = snapshot.revision;
        let playback = to_playback_snapshot(snapshot, next_revision)?;
        self.remote_sequence = Some(remote_revision);
        self.application_revision = Some(next_revision);
        Ok(Some(playback))
    }

    #[must_use]
    pub const fn current_session(&self) -> Option<Uuid> {
        self.session_id
    }

    #[must_use]
    pub fn next_disconnect_revision(&mut self) -> u64 {
        let revision = self
            .application_revision
            .unwrap_or_default()
            .saturating_add(1);
        self.application_revision = Some(revision);
        revision
    }
}

fn to_playback_snapshot(snapshot: StateSnapshot, revision: u64) -> AppResult<PlaybackSnapshot> {
    let volume = normalize_wire_volume(snapshot.volume)?;
    Ok(PlaybackSnapshot {
        revision,
        status: snapshot.status,
        track_id: snapshot.track_id,
        position_ms: snapshot.position_ms,
        duration_ms: snapshot.duration_ms,
        volume,
        backend: snapshot.backend,
    })
}

fn normalize_wire_volume(volume: f64) -> AppResult<f32> {
    if !volume.is_finite() || !(0.0..=1.0).contains(&volume) {
        return Err(playback_error("invalid_playback_volume"));
    }
    Ok(volume as f32)
}
