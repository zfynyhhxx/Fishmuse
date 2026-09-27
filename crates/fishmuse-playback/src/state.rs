use fishmuse_domain::{AppError, AppResult, ErrorCategory, ErrorCode};
use serde::{Deserialize, Serialize};

use crate::{PlaybackCommand, PlaybackEvent, PlaybackSnapshot};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackStatus {
    Stopped,
    Loading,
    Playing,
    Paused,
    Unavailable,
}

#[derive(Clone, Debug)]
pub struct PlaybackStateMachine {
    snapshot: PlaybackSnapshot,
}

impl PlaybackStateMachine {
    pub fn new(snapshot: PlaybackSnapshot) -> AppResult<Self> {
        validate_snapshot(&snapshot)?;
        Ok(Self { snapshot })
    }

    #[must_use]
    pub const fn snapshot(&self) -> &PlaybackSnapshot {
        &self.snapshot
    }

    pub fn apply(&mut self, event: PlaybackEvent) -> AppResult<bool> {
        if event.revision() <= self.snapshot.revision {
            return Ok(false);
        }
        let next = event.into_snapshot();
        validate_snapshot(&next)?;
        if !valid_transition(self.snapshot.status, next.status) {
            return Err(playback_error("invalid_playback_transition"));
        }
        self.snapshot = next;
        Ok(true)
    }

    pub fn validate_command(
        &self,
        command: &PlaybackCommand,
        source_available: bool,
    ) -> AppResult<()> {
        match command {
            PlaybackCommand::Play { source, .. } => {
                if !source_available {
                    return Err(playback_error("playback_source_unavailable"));
                }
                if matches!((source.start_ms, source.end_ms), (Some(start), Some(end)) if start >= end)
                {
                    return Err(playback_error("invalid_playback_source"));
                }
            }
            PlaybackCommand::Pause { .. } if self.snapshot.status != PlaybackStatus::Playing => {
                return Err(playback_error("invalid_playback_transition"));
            }
            PlaybackCommand::Resume { .. } if self.snapshot.status != PlaybackStatus::Paused => {
                return Err(playback_error("invalid_playback_transition"));
            }
            PlaybackCommand::Seek { position_ms, .. } => {
                if !matches!(
                    self.snapshot.status,
                    PlaybackStatus::Playing | PlaybackStatus::Paused
                ) {
                    return Err(playback_error("invalid_playback_transition"));
                }
                if self
                    .snapshot
                    .duration_ms
                    .is_some_and(|duration| *position_ms > duration)
                {
                    return Err(playback_error("seek_out_of_bounds"));
                }
            }
            PlaybackCommand::SkipNext { .. }
                if !matches!(
                    self.snapshot.status,
                    PlaybackStatus::Playing | PlaybackStatus::Paused
                ) =>
            {
                return Err(playback_error("invalid_playback_transition"));
            }
            _ => {}
        }
        Ok(())
    }
}

fn validate_snapshot(snapshot: &PlaybackSnapshot) -> AppResult<()> {
    if snapshot
        .duration_ms
        .is_some_and(|duration| snapshot.position_ms > duration)
    {
        return Err(playback_error("seek_out_of_bounds"));
    }
    if matches!(
        snapshot.status,
        PlaybackStatus::Playing | PlaybackStatus::Paused
    ) && snapshot.track_id.is_none()
    {
        return Err(playback_error("invalid_playback_state"));
    }
    Ok(())
}

fn valid_transition(current: PlaybackStatus, next: PlaybackStatus) -> bool {
    current == next
        || matches!(
            (current, next),
            (PlaybackStatus::Stopped, PlaybackStatus::Loading)
                | (PlaybackStatus::Stopped, PlaybackStatus::Unavailable)
                | (PlaybackStatus::Loading, PlaybackStatus::Playing)
                | (PlaybackStatus::Loading, PlaybackStatus::Stopped)
                | (PlaybackStatus::Loading, PlaybackStatus::Unavailable)
                | (PlaybackStatus::Playing, PlaybackStatus::Paused)
                | (PlaybackStatus::Playing, PlaybackStatus::Stopped)
                | (PlaybackStatus::Playing, PlaybackStatus::Loading)
                | (PlaybackStatus::Playing, PlaybackStatus::Unavailable)
                | (PlaybackStatus::Paused, PlaybackStatus::Playing)
                | (PlaybackStatus::Paused, PlaybackStatus::Stopped)
                | (PlaybackStatus::Paused, PlaybackStatus::Loading)
                | (PlaybackStatus::Paused, PlaybackStatus::Unavailable)
                | (PlaybackStatus::Unavailable, PlaybackStatus::Stopped)
                | (PlaybackStatus::Unavailable, PlaybackStatus::Loading)
        )
}

pub(crate) fn playback_error(message: &str) -> AppError {
    AppError {
        code: if message == "playback_source_unavailable" {
            ErrorCode::Unavailable
        } else {
            ErrorCode::InvalidInput
        },
        category: ErrorCategory::Playback,
        user_message: message.to_owned(),
        retryable: false,
        suggested_action: None,
        technical_context: None,
    }
}
