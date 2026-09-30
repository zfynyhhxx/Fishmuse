use std::{collections::VecDeque, sync::Arc};

use async_trait::async_trait;
use fishmuse_domain::{
    AppError, AppResult, ErrorCategory, ErrorCode, OperationId, PlayableSource, TrackId, UserId,
};
use fishmuse_library::LibraryQueryPort;
use fishmuse_playback::{PlaybackCommand, PlaybackControl, PlaybackEvent, PlaybackSnapshot};
use tokio::sync::{Mutex, broadcast};

use crate::state::PlaybackApplicationService;

const MAX_QUEUE_TRACKS: usize = 1_000;
const MAX_HISTORY_TRACKS: usize = 100;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QueueCommand {
    PlayNow {
        track_id: TrackId,
        context: Vec<TrackId>,
        operation_id: OperationId,
    },
    Add {
        track_id: TrackId,
    },
    PlayAt {
        index: usize,
    },
    Remove {
        index: usize,
    },
    Clear,
    Previous,
    Next,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueueSnapshot {
    pub track_ids: Vec<TrackId>,
    pub current_index: Option<usize>,
    pub can_previous: bool,
    pub can_next: bool,
}

#[derive(Default)]
struct QueueState {
    track_ids: Vec<TrackId>,
    current_index: Option<usize>,
    history: VecDeque<TrackId>,
}

impl QueueState {
    fn snapshot(&self) -> QueueSnapshot {
        QueueSnapshot {
            track_ids: self.track_ids.clone(),
            current_index: self.current_index,
            can_previous: self.current_index.is_some(),
            can_next: self
                .current_index
                .is_none_or(|index| index.saturating_add(1) < self.track_ids.len()),
        }
    }

    fn remember_current(&mut self) {
        let Some(track_id) = self
            .current_index
            .and_then(|index| self.track_ids.get(index))
            .copied()
        else {
            return;
        };
        if self.history.len() == MAX_HISTORY_TRACKS {
            self.history.pop_front();
        }
        self.history.push_back(track_id);
    }
}

pub struct PlaybackQueueService {
    user_id: UserId,
    library: Arc<dyn LibraryQueryPort>,
    application: Arc<dyn PlaybackApplicationService>,
    state: Mutex<QueueState>,
}

impl PlaybackQueueService {
    #[must_use]
    pub fn new<L, A>(user_id: UserId, library: Arc<L>, application: Arc<A>) -> Self
    where
        L: LibraryQueryPort + 'static,
        A: PlaybackApplicationService + 'static,
    {
        Self {
            user_id,
            library,
            application,
            state: Mutex::new(QueueState::default()),
        }
    }

    pub async fn apply(&self, command: QueueCommand) -> AppResult<QueueSnapshot> {
        let mut state = self.state.lock().await;
        match command {
            QueueCommand::PlayNow {
                track_id,
                context,
                operation_id,
            } => {
                validate_context(&context, track_id)?;
                let index = context
                    .iter()
                    .position(|candidate| *candidate == track_id)
                    .expect("validated context contains selected track");
                let source = self.resolve(track_id).await?;
                self.application
                    .execute(PlaybackCommand::Play {
                        source,
                        operation_id,
                    })
                    .await?;
                state.track_ids = context;
                state.current_index = Some(index);
                state.history.clear();
            }
            QueueCommand::Add { track_id } => {
                if state.track_ids.len() == MAX_QUEUE_TRACKS {
                    return Err(queue_error(
                        ErrorCode::InvalidInput,
                        "The playback queue can contain at most 1000 tracks.",
                    ));
                }
                state.track_ids.push(track_id);
            }
            QueueCommand::PlayAt { index } => {
                self.play_index(&mut state, index, OperationId::new())
                    .await?;
            }
            QueueCommand::Remove { index } => {
                if index >= state.track_ids.len() {
                    return Err(queue_error(
                        ErrorCode::InvalidInput,
                        "The selected queue position is no longer available.",
                    ));
                }
                let removed = state.track_ids.remove(index);
                state.history.retain(|track_id| *track_id != removed);
                state.current_index = match state.current_index {
                    Some(current) if index < current => Some(current - 1),
                    Some(current) if index == current => None,
                    current => current,
                };
            }
            QueueCommand::Clear => {
                state.track_ids.clear();
                state.current_index = None;
                state.history.clear();
            }
            QueueCommand::Previous => {
                self.previous(&mut state, OperationId::new()).await?;
            }
            QueueCommand::Next => {
                self.next(&mut state, OperationId::new()).await?;
            }
        }
        Ok(state.snapshot())
    }

    pub async fn queue_snapshot(&self) -> QueueSnapshot {
        self.state.lock().await.snapshot()
    }

    async fn resolve(&self, track_id: TrackId) -> AppResult<PlayableSource> {
        self.library
            .playable_source(self.user_id, track_id)
            .await?
            .ok_or_else(|| {
                queue_error(ErrorCode::NotFound, "This track is not currently playable.")
            })
    }

    async fn play_index(
        &self,
        state: &mut QueueState,
        index: usize,
        operation_id: OperationId,
    ) -> AppResult<PlaybackSnapshot> {
        let track_id = state.track_ids.get(index).copied().ok_or_else(|| {
            queue_error(
                ErrorCode::InvalidInput,
                "The selected queue position is no longer available.",
            )
        })?;
        let source = self.resolve(track_id).await?;
        let snapshot = self
            .application
            .execute(PlaybackCommand::Play {
                source,
                operation_id,
            })
            .await?;
        if state.current_index != Some(index) {
            state.remember_current();
        }
        state.current_index = Some(index);
        Ok(snapshot)
    }

    async fn previous(
        &self,
        state: &mut QueueState,
        operation_id: OperationId,
    ) -> AppResult<PlaybackSnapshot> {
        if let Some(track_id) = state.history.back().copied() {
            let source = self.resolve(track_id).await?;
            let snapshot = self
                .application
                .execute(PlaybackCommand::Play {
                    source,
                    operation_id,
                })
                .await?;
            state.history.pop_back();
            state.current_index = state
                .track_ids
                .iter()
                .position(|candidate| *candidate == track_id);
            return Ok(snapshot);
        }
        if state.current_index.is_none() {
            return Err(queue_error(
                ErrorCode::InvalidInput,
                "There is no current track to restart.",
            ));
        }
        self.application
            .execute(PlaybackCommand::Seek {
                position_ms: 0,
                operation_id,
            })
            .await
    }

    async fn next(
        &self,
        state: &mut QueueState,
        operation_id: OperationId,
    ) -> AppResult<PlaybackSnapshot> {
        let next_index = state
            .current_index
            .map_or(0, |index| index.saturating_add(1));
        if next_index < state.track_ids.len() {
            return self.play_index(state, next_index, operation_id).await;
        }
        self.application
            .execute(PlaybackCommand::Stop { operation_id })
            .await
    }
}

#[async_trait]
impl PlaybackControl for PlaybackQueueService {
    async fn execute(&self, command: PlaybackCommand) -> AppResult<PlaybackSnapshot> {
        let mut state = self.state.lock().await;
        match command {
            PlaybackCommand::Play {
                source,
                operation_id,
            } => {
                let track_id = source.track_id;
                let snapshot = self
                    .application
                    .execute(PlaybackCommand::Play {
                        source,
                        operation_id,
                    })
                    .await?;
                state.track_ids = vec![track_id];
                state.current_index = Some(0);
                state.history.clear();
                Ok(snapshot)
            }
            PlaybackCommand::SkipNext { operation_id } => self.next(&mut state, operation_id).await,
            command => self.application.execute(command).await,
        }
    }

    async fn snapshot(&self) -> AppResult<PlaybackSnapshot> {
        self.application.snapshot().await
    }

    fn subscribe(&self) -> broadcast::Receiver<PlaybackEvent> {
        self.application
            .subscribe()
            .expect("production playback application publishes events")
    }
}

#[async_trait]
impl PlaybackApplicationService for PlaybackQueueService {
    async fn retry(&self) -> AppResult<()> {
        self.application.retry().await
    }

    async fn execute(&self, command: PlaybackCommand) -> AppResult<PlaybackSnapshot> {
        PlaybackControl::execute(self, command).await
    }

    async fn snapshot(&self) -> AppResult<PlaybackSnapshot> {
        PlaybackControl::snapshot(self).await
    }

    async fn shutdown(&self) {
        self.application.shutdown().await;
    }

    fn subscribe(&self) -> Option<broadcast::Receiver<PlaybackEvent>> {
        Some(PlaybackControl::subscribe(self))
    }

    fn subscribe_service_state(
        &self,
    ) -> Option<tokio::sync::watch::Receiver<fishmuse_playback::PlaybackServiceState>> {
        self.application.subscribe_service_state()
    }

    async fn apply_queue(&self, command: QueueCommand) -> AppResult<QueueSnapshot> {
        self.apply(command).await
    }

    async fn queue_snapshot(&self) -> AppResult<QueueSnapshot> {
        Ok(self.queue_snapshot().await)
    }
}

fn validate_context(context: &[TrackId], track_id: TrackId) -> AppResult<()> {
    if context.is_empty() || context.len() > MAX_QUEUE_TRACKS {
        return Err(queue_error(
            ErrorCode::InvalidInput,
            "The playback queue must contain between 1 and 1000 tracks.",
        ));
    }
    if !context.contains(&track_id) {
        return Err(queue_error(
            ErrorCode::InvalidInput,
            "The selected track is not present in the queue context.",
        ));
    }
    Ok(())
}

fn queue_error(code: ErrorCode, message: &str) -> AppError {
    AppError {
        code,
        category: ErrorCategory::Playback,
        user_message: message.to_owned(),
        retryable: false,
        suggested_action: None,
        technical_context: None,
    }
}
