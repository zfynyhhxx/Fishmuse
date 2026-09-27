use async_trait::async_trait;
use fishmuse_domain::{AppResult, ListenId, TrackId};
use time::OffsetDateTime;

use crate::{PlaybackEvent, PlaybackStatus};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListeningEvent {
    pub id: ListenId,
    pub track_id: TrackId,
    pub started_at: OffsetDateTime,
    pub ended_at: Option<OffsetDateTime>,
    pub listened_ms: u64,
    pub completed: bool,
    pub interrupted: bool,
}

#[async_trait]
pub trait ListeningSink: Send + Sync {
    async fn append(&self, event: ListeningEvent) -> AppResult<()>;

    async fn recover_interrupted(&self, _ended_at: OffsetDateTime) -> AppResult<()> {
        Ok(())
    }
}

pub trait Clock: Send + Sync {
    fn now(&self) -> OffsetDateTime;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> OffsetDateTime {
        OffsetDateTime::now_utc()
    }
}

pub struct ListenTracker<S, C> {
    sink: S,
    clock: C,
    last_revision: Option<u64>,
    active: Option<ActiveListen>,
}

struct ActiveListen {
    id: ListenId,
    track_id: TrackId,
    started_at: OffsetDateTime,
    running_since: Option<OffsetDateTime>,
    listened_ms: u64,
    duration_ms: Option<u64>,
}

impl<S, C> ListenTracker<S, C>
where
    S: ListeningSink,
    C: Clock,
{
    pub async fn start(sink: S, clock: C) -> AppResult<Self> {
        sink.recover_interrupted(clock.now()).await?;
        Ok(Self {
            sink,
            clock,
            last_revision: None,
            active: None,
        })
    }

    pub async fn handle(&mut self, event: PlaybackEvent) -> AppResult<bool> {
        let revision = event.revision();
        if self
            .last_revision
            .is_some_and(|last_revision| revision <= last_revision)
        {
            return Ok(false);
        }
        let disconnected = matches!(event, PlaybackEvent::Disconnected { .. });
        let snapshot = event.into_snapshot();
        let now = self.clock.now();

        match snapshot.status {
            PlaybackStatus::Playing => {
                if let Some(track_id) = snapshot.track_id {
                    if self
                        .active
                        .as_ref()
                        .is_some_and(|active| active.track_id != track_id)
                    {
                        self.settle(now, false).await?;
                    }
                    if self.active.is_some() {
                        self.sync_running(now);
                        if let Some(active) = &mut self.active {
                            active.duration_ms = snapshot.duration_ms.or(active.duration_ms);
                            if active.running_since.is_none() {
                                active.running_since = Some(now);
                            }
                        }
                    } else {
                        self.active = Some(ActiveListen {
                            id: ListenId::new(),
                            track_id,
                            started_at: now,
                            running_since: Some(now),
                            listened_ms: 0,
                            duration_ms: snapshot.duration_ms,
                        });
                    }
                    self.persist_active(None, false).await?;
                }
            }
            PlaybackStatus::Paused => {
                if self
                    .active
                    .as_ref()
                    .is_some_and(|active| Some(active.track_id) == snapshot.track_id)
                {
                    self.pause(now);
                    if let Some(active) = &mut self.active {
                        active.duration_ms = snapshot.duration_ms.or(active.duration_ms);
                    }
                    self.persist_active(None, false).await?;
                } else if self.active.is_some() {
                    self.settle(now, false).await?;
                }
            }
            PlaybackStatus::Stopped => self.settle(now, false).await?,
            PlaybackStatus::Unavailable => self.settle(now, true).await?,
            PlaybackStatus::Loading => {
                if self.active.as_ref().is_some_and(|active| {
                    snapshot
                        .track_id
                        .is_none_or(|track_id| track_id != active.track_id)
                }) {
                    self.settle(now, false).await?;
                }
            }
        }

        if disconnected && self.active.is_some() {
            self.settle(now, true).await?;
        }
        self.last_revision = Some(revision);
        Ok(true)
    }

    fn sync_running(&mut self, now: OffsetDateTime) {
        let Some(active) = &mut self.active else {
            return;
        };
        let Some(running_since) = active.running_since.replace(now) else {
            return;
        };
        active.listened_ms = active
            .listened_ms
            .saturating_add(elapsed_ms(running_since, now));
    }

    fn pause(&mut self, now: OffsetDateTime) {
        let Some(active) = &mut self.active else {
            return;
        };
        let Some(running_since) = active.running_since.take() else {
            return;
        };
        active.listened_ms = active
            .listened_ms
            .saturating_add(elapsed_ms(running_since, now));
    }

    async fn settle(&mut self, now: OffsetDateTime, interrupted: bool) -> AppResult<()> {
        if self.active.is_none() {
            return Ok(());
        }
        self.pause(now);
        self.persist_active(Some(now), interrupted).await?;
        self.active = None;
        Ok(())
    }

    async fn persist_active(
        &self,
        ended_at: Option<OffsetDateTime>,
        interrupted: bool,
    ) -> AppResult<()> {
        let Some(active) = &self.active else {
            return Ok(());
        };
        self.sink
            .append(ListeningEvent {
                id: active.id,
                track_id: active.track_id,
                started_at: active.started_at,
                ended_at,
                listened_ms: active.listened_ms,
                completed: active.listened_ms >= completion_threshold(active.duration_ms),
                interrupted,
            })
            .await
    }
}

fn elapsed_ms(start: OffsetDateTime, end: OffsetDateTime) -> u64 {
    u64::try_from((end - start).whole_milliseconds()).unwrap_or(0)
}

fn completion_threshold(duration_ms: Option<u64>) -> u64 {
    duration_ms
        .map(|duration| duration / 2 + duration % 2)
        .unwrap_or(240_000)
        .min(240_000)
}
