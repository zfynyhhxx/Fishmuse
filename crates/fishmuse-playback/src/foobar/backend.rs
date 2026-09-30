use std::{
    path::PathBuf,
    sync::{
        Arc, RwLock,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use fishmuse_domain::{AppError, AppResult, ErrorCategory, ErrorCode, MediaAssetId, OperationId};
use tokio::sync::{Mutex, Notify, broadcast, watch};
use uuid::Uuid;

use crate::{
    PlaybackBackend, PlaybackBackendKind, PlaybackCommand, PlaybackEvent, PlaybackSnapshot,
    PlaybackStatus,
};

use super::{
    client::{FoobarClient, unavailable},
    protocol::{StateSnapshot, WireCommand},
    reconcile::StateReconciler,
};

#[async_trait]
pub trait MediaPathResolver: Send + Sync {
    async fn resolve(&self, media_asset_id: MediaAssetId) -> AppResult<PathBuf>;
}

struct MissingPathResolver;

#[async_trait]
impl MediaPathResolver for MissingPathResolver {
    async fn resolve(&self, _media_asset_id: MediaAssetId) -> AppResult<PathBuf> {
        Err(AppError {
            code: ErrorCode::NotFound,
            category: ErrorCategory::Playback,
            user_message: "The selected media source is unavailable.".to_owned(),
            retryable: false,
            suggested_action: Some("Rescan the local music library.".to_owned()),
            technical_context: Some("media_path_resolver_not_configured".to_owned()),
        })
    }
}

#[derive(Clone)]
pub struct FoobarConfig {
    pipe_name: Arc<str>,
    ack_timeout: Duration,
    handshake_timeout: Duration,
    retry_wait: Duration,
    writer_capacity: usize,
    event_capacity: usize,
    reconnect_policy: ReconnectPolicy,
    media_paths: Arc<dyn MediaPathResolver>,
}

impl FoobarConfig {
    #[must_use]
    pub fn for_pipe(pipe_name: impl Into<String>) -> Self {
        Self {
            pipe_name: Arc::from(pipe_name.into()),
            ack_timeout: Duration::from_millis(500),
            handshake_timeout: Duration::from_millis(500),
            retry_wait: Duration::from_secs(2),
            writer_capacity: 64,
            event_capacity: 64,
            reconnect_policy: ReconnectPolicy::default(),
            media_paths: Arc::new(MissingPathResolver),
        }
    }

    #[must_use]
    pub fn with_ack_timeout(mut self, timeout: Duration) -> Self {
        self.ack_timeout = timeout;
        self
    }

    #[must_use]
    pub fn with_handshake_timeout(mut self, timeout: Duration) -> Self {
        self.handshake_timeout = timeout;
        self
    }

    #[must_use]
    pub fn with_retry_wait(mut self, timeout: Duration) -> Self {
        self.retry_wait = timeout;
        self
    }

    #[must_use]
    pub fn with_reconnect_policy(mut self, policy: ReconnectPolicy) -> Self {
        self.reconnect_policy = policy;
        self
    }

    #[must_use]
    pub fn with_media_path_resolver<R>(mut self, resolver: Arc<R>) -> Self
    where
        R: MediaPathResolver + 'static,
    {
        self.media_paths = resolver;
        self
    }

    pub(crate) fn pipe_name(&self) -> &str {
        &self.pipe_name
    }

    pub(crate) const fn ack_timeout(&self) -> Duration {
        self.ack_timeout
    }

    pub(crate) const fn handshake_timeout(&self) -> Duration {
        self.handshake_timeout
    }

    pub(crate) const fn writer_capacity(&self) -> usize {
        self.writer_capacity
    }

    pub(crate) const fn event_capacity(&self) -> usize {
        self.event_capacity
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReconnectPolicy {
    base_delay: Duration,
    maximum_delay: Duration,
    jitter_percent: u8,
}

impl ReconnectPolicy {
    #[must_use]
    pub const fn new(base_delay: Duration, maximum_delay: Duration, jitter_percent: u8) -> Self {
        Self {
            base_delay,
            maximum_delay,
            jitter_percent,
        }
    }

    #[must_use]
    pub fn delay_for(self, attempt: u32, jitter_sample: u8) -> Duration {
        let shift = attempt.min(63);
        let multiplier = 1_u128 << shift;
        let base_ms = self
            .base_delay
            .as_millis()
            .saturating_mul(multiplier)
            .min(self.maximum_delay.as_millis());
        let jitter = u128::from(self.jitter_percent.min(100));
        let range = base_ms.saturating_mul(jitter) / 100;
        let sample = u128::from(jitter_sample.min(100));
        let positive = range.saturating_mul(sample).saturating_mul(2) / 100;
        let adjusted = base_ms.saturating_sub(range).saturating_add(positive);
        Duration::from_millis(u64::try_from(adjusted).unwrap_or(u64::MAX))
    }
}

impl Default for ReconnectPolicy {
    fn default() -> Self {
        Self::new(Duration::from_millis(100), Duration::from_secs(5), 20)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConnectionState {
    Connecting { attempt: u32 },
    Connected { session_id: Uuid },
    Disconnected,
    Unavailable,
}

#[derive(Clone)]
pub struct FoobarBackend {
    inner: Arc<BackendInner>,
}

struct BackendInner {
    config: FoobarConfig,
    client: tokio::sync::RwLock<Option<Arc<FoobarClient>>>,
    connection: watch::Sender<ConnectionState>,
    events: broadcast::Sender<PlaybackEvent>,
    snapshot: RwLock<Option<PlaybackSnapshot>>,
    reconcile: Mutex<StateReconciler>,
    reconnect: Notify,
    shutdown: watch::Sender<bool>,
    entropy: AtomicU64,
}

impl FoobarBackend {
    pub async fn connect(config: FoobarConfig) -> AppResult<Self> {
        validate_config(&config)?;
        let (connection, _) = watch::channel(ConnectionState::Connecting { attempt: 0 });
        let (events, _) = broadcast::channel(config.event_capacity);
        let (shutdown, _) = watch::channel(false);
        let backend = Self {
            inner: Arc::new(BackendInner {
                config,
                client: tokio::sync::RwLock::new(None),
                connection,
                events,
                snapshot: RwLock::new(None),
                reconcile: Mutex::new(StateReconciler::default()),
                reconnect: Notify::new(),
                shutdown,
                entropy: AtomicU64::new(0x9e37_79b9_7f4a_7c15),
            }),
        };
        tokio::spawn(supervisor(backend.inner.clone()));
        Ok(backend)
    }

    #[must_use]
    pub fn connection_state(&self) -> watch::Receiver<ConnectionState> {
        self.inner.connection.subscribe()
    }

    pub fn reconnect_now(&self) {
        self.inner.reconnect.notify_one();
    }

    pub async fn shutdown(&self) {
        let mut state = self.connection_state();
        let _ = self.inner.shutdown.send(true);
        while !matches!(*state.borrow(), ConnectionState::Disconnected) {
            if state.changed().await.is_err() {
                break;
            }
        }
    }

    async fn execute_wire(
        &self,
        operation_id: OperationId,
        command: WireCommand,
    ) -> AppResult<PlaybackSnapshot> {
        let client = self
            .inner
            .client
            .read()
            .await
            .clone()
            .ok_or_else(|| unavailable("backend_not_connected"))?;
        let previous_session = client.session_id();
        match client.command(operation_id, command.clone()).await {
            Ok(ack) => self.accept_incremental(ack.snapshot).await,
            Err(error) if error.retryable => {
                self.reconnect_now();
                let replacement = self.wait_for_replacement(previous_session).await?;
                let ack = replacement.command(operation_id, command).await?;
                self.accept_incremental(ack.snapshot).await
            }
            Err(error) => Err(error),
        }
    }

    async fn wait_for_replacement(&self, previous_session: Uuid) -> AppResult<Arc<FoobarClient>> {
        let mut state = self.connection_state();
        let wait = async {
            loop {
                let replacement = if matches!(
                    *state.borrow(),
                    ConnectionState::Connected { session_id } if session_id != previous_session
                ) {
                    self.inner
                        .client
                        .read()
                        .await
                        .clone()
                        .filter(|client| client.session_id() != previous_session)
                } else {
                    None
                };
                if let Some(client) = replacement {
                    return Ok(client);
                }
                state
                    .changed()
                    .await
                    .map_err(|_| unavailable("connection_state_closed"))?;
            }
        };
        tokio::time::timeout(self.inner.config.retry_wait, wait)
            .await
            .map_err(|_| unavailable("reconnect_timeout"))?
    }

    async fn accept_incremental(&self, raw: StateSnapshot) -> AppResult<PlaybackSnapshot> {
        let mut reconcile = self.inner.reconcile.lock().await;
        let accepted = reconcile.apply_event(raw)?;
        drop(reconcile);
        if let Some(snapshot) = accepted {
            self.publish_snapshot(snapshot.clone());
            Ok(snapshot)
        } else {
            self.cached_snapshot()
                .ok_or_else(|| unavailable("stale_snapshot_without_cache"))
        }
    }

    fn publish_snapshot(&self, snapshot: PlaybackSnapshot) {
        *self
            .inner
            .snapshot
            .write()
            .expect("foobar backend snapshot lock poisoned") = Some(snapshot.clone());
        let _ = self.inner.events.send(PlaybackEvent::Snapshot(snapshot));
    }

    fn cached_snapshot(&self) -> Option<PlaybackSnapshot> {
        self.inner
            .snapshot
            .read()
            .expect("foobar backend snapshot lock poisoned")
            .clone()
    }
}

#[async_trait]
impl PlaybackBackend for FoobarBackend {
    async fn execute(&self, command: PlaybackCommand) -> AppResult<PlaybackSnapshot> {
        match command {
            PlaybackCommand::Play {
                source,
                operation_id,
            } => {
                let path = self
                    .inner
                    .config
                    .media_paths
                    .resolve(source.media_asset_id)
                    .await?;
                let path = path.to_str().ok_or_else(|| AppError {
                    code: ErrorCode::InvalidInput,
                    category: ErrorCategory::Playback,
                    user_message: "The selected media path is not valid Unicode.".to_owned(),
                    retryable: false,
                    suggested_action: Some("Move the file to a valid Windows path.".to_owned()),
                    technical_context: Some("non_unicode_media_path".to_owned()),
                })?;
                self.execute_wire(
                    operation_id,
                    WireCommand::Play {
                        track_id: source.track_id,
                        path: path.to_owned(),
                        subsong_index: source.subsong_index,
                        start_ms: source.start_ms,
                        end_ms: source.end_ms,
                    },
                )
                .await
            }
            PlaybackCommand::Pause { operation_id } => {
                self.execute_wire(operation_id, WireCommand::Pause).await
            }
            PlaybackCommand::Resume { operation_id } => {
                self.execute_wire(operation_id, WireCommand::Resume).await
            }
            command @ (PlaybackCommand::Stop { .. } | PlaybackCommand::SetVolume { .. }) => {
                let operation_id = command.operation_id();
                let wire_command = control_wire_command(command)?;
                self.execute_wire(operation_id, wire_command).await
            }
            PlaybackCommand::Seek {
                position_ms,
                operation_id,
            } => {
                self.execute_wire(operation_id, WireCommand::Seek { position_ms })
                    .await
            }
            PlaybackCommand::SkipNext { operation_id } => {
                self.execute_wire(operation_id, WireCommand::SkipNext).await
            }
        }
    }

    async fn snapshot(&self) -> AppResult<PlaybackSnapshot> {
        self.execute_wire(OperationId::new(), WireCommand::GetState)
            .await
    }

    fn subscribe(&self) -> broadcast::Receiver<PlaybackEvent> {
        self.inner.events.subscribe()
    }
}

fn control_wire_command(command: PlaybackCommand) -> AppResult<WireCommand> {
    match command {
        PlaybackCommand::Stop { .. } => Ok(WireCommand::Stop),
        PlaybackCommand::SetVolume { volume, .. }
            if volume.is_finite() && (0.0..=1.0).contains(&volume) =>
        {
            Ok(WireCommand::SetVolume {
                volume: f64::from(volume),
            })
        }
        PlaybackCommand::SetVolume { .. } => {
            Err(crate::state::playback_error("invalid_playback_volume"))
        }
        _ => unreachable!("only provider-neutral control commands are mapped here"),
    }
}

impl Drop for FoobarBackend {
    fn drop(&mut self) {
        if Arc::strong_count(&self.inner) == 2 {
            let _ = self.inner.shutdown.send(true);
        }
    }
}

async fn supervisor(inner: Arc<BackendInner>) {
    let mut attempt = 0_u32;
    let mut shutdown = inner.shutdown.subscribe();
    loop {
        if *shutdown.borrow() {
            let _ = inner.connection.send(ConnectionState::Disconnected);
            return;
        }
        let _ = inner
            .connection
            .send(ConnectionState::Connecting { attempt });
        let connect = FoobarClient::connect(inner.config.clone());
        tokio::pin!(connect);
        let outcome = tokio::select! {
            changed = shutdown.changed() => {
                let _ = changed;
                let _ = inner.connection.send(ConnectionState::Disconnected);
                return;
            }
            result = &mut connect => result,
        };
        match outcome {
            Ok(client) => {
                let client = Arc::new(client);
                let session_id = client.session_id();
                let initial = {
                    let mut reconcile = inner.reconcile.lock().await;
                    reconcile.begin_session(client.snapshot())
                };
                let Ok(initial) = initial else {
                    let _ = inner.connection.send(ConnectionState::Unavailable);
                    if !wait_before_retry(&inner, attempt, &mut shutdown).await {
                        let _ = inner.connection.send(ConnectionState::Disconnected);
                        return;
                    }
                    attempt = attempt.saturating_add(1);
                    continue;
                };
                *inner
                    .snapshot
                    .write()
                    .expect("foobar backend snapshot lock poisoned") = Some(initial.clone());
                let _ = inner.events.send(PlaybackEvent::Snapshot(initial));
                *inner.client.write().await = Some(client.clone());
                let _ = inner
                    .connection
                    .send(ConnectionState::Connected { session_id });
                attempt = 0;

                let mut events = client.subscribe();
                let mut disconnected = client.disconnected();
                let mut shutdown_requested = false;
                loop {
                    tokio::select! {
                        changed = shutdown.changed() => {
                            let _ = changed;
                            shutdown_requested = true;
                            break;
                        }
                        _ = inner.reconnect.notified() => break,
                        changed = disconnected.changed() => {
                            if changed.is_err() || *disconnected.borrow() {
                                while let Ok(event) = events.try_recv() {
                                    accept_client_event(&inner, session_id, event).await;
                                }
                                break;
                            }
                        }
                        event = events.recv() => match event {
                            Ok(event) => accept_client_event(&inner, session_id, event).await,
                            Err(broadcast::error::RecvError::Lagged(_)) => break,
                            Err(broadcast::error::RecvError::Closed) => break,
                        }
                    }
                }
                let mut active = inner.client.write().await;
                if active
                    .as_ref()
                    .is_some_and(|value| value.session_id() == session_id)
                {
                    active.take();
                }
                drop(active);
                mark_disconnected(&inner).await;
                if shutdown_requested {
                    return;
                }
            }
            Err(_) => {
                *inner.client.write().await = None;
                let _ = inner.connection.send(ConnectionState::Unavailable);
            }
        }

        if !wait_before_retry(&inner, attempt, &mut shutdown).await {
            let _ = inner.connection.send(ConnectionState::Disconnected);
            return;
        }
        attempt = attempt.saturating_add(1);
    }
}

async fn accept_client_event(
    inner: &BackendInner,
    session_id: Uuid,
    event: super::client::ClientEvent,
) {
    if event.session_id != session_id || event.sequence != event.snapshot.revision {
        return;
    }
    let accepted = {
        let mut reconcile = inner.reconcile.lock().await;
        reconcile.apply_event(event.snapshot).ok().flatten()
    };
    if let Some(snapshot) = accepted {
        *inner
            .snapshot
            .write()
            .expect("foobar backend snapshot lock poisoned") = Some(snapshot.clone());
        let _ = inner.events.send(PlaybackEvent::Snapshot(snapshot));
    }
}

async fn mark_disconnected(inner: &BackendInner) {
    let revision = inner.reconcile.lock().await.next_disconnect_revision();
    let snapshot = PlaybackSnapshot {
        revision,
        status: PlaybackStatus::Unavailable,
        track_id: None,
        position_ms: 0,
        duration_ms: None,
        volume: 1.0,
        backend: PlaybackBackendKind::Foobar2000,
    };
    *inner
        .snapshot
        .write()
        .expect("foobar backend snapshot lock poisoned") = Some(snapshot);
    let _ = inner.events.send(PlaybackEvent::Disconnected {
        revision,
        backend: PlaybackBackendKind::Foobar2000,
    });
    let _ = inner.connection.send(ConnectionState::Disconnected);
}

async fn wait_before_retry(
    inner: &BackendInner,
    attempt: u32,
    shutdown: &mut watch::Receiver<bool>,
) -> bool {
    let sample = next_jitter_sample(&inner.entropy);
    let delay = inner.config.reconnect_policy.delay_for(attempt, sample);
    tokio::select! {
        () = tokio::time::sleep(delay) => true,
        _ = inner.reconnect.notified() => true,
        changed = shutdown.changed() => {
            let _ = changed;
            false
        }
    }
}

fn next_jitter_sample(state: &AtomicU64) -> u8 {
    let mut current = state.load(Ordering::Relaxed);
    loop {
        let mut next = current;
        next ^= next << 13;
        next ^= next >> 7;
        next ^= next << 17;
        match state.compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return u8::try_from(next % 101).expect("jitter sample fits u8"),
            Err(actual) => current = actual,
        }
    }
}

fn validate_config(config: &FoobarConfig) -> AppResult<()> {
    if !config.pipe_name.starts_with(r"\\.\pipe\")
        || config.ack_timeout.is_zero()
        || config.handshake_timeout.is_zero()
        || config.retry_wait.is_zero()
        || config.writer_capacity == 0
        || config.event_capacity == 0
        || config.reconnect_policy.base_delay.is_zero()
        || config.reconnect_policy.maximum_delay < config.reconnect_policy.base_delay
        || config.reconnect_policy.jitter_percent > 100
    {
        return Err(AppError {
            code: ErrorCode::InvalidInput,
            category: ErrorCategory::Configuration,
            user_message: "The foobar2000 bridge configuration is invalid.".to_owned(),
            retryable: false,
            suggested_action: None,
            technical_context: Some("invalid_foobar_config".to_owned()),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use fishmuse_domain::OperationId;

    use super::{PlaybackCommand, WireCommand, control_wire_command};

    #[test]
    fn stop_and_volume_map_to_frozen_v1_wire_commands() {
        assert_eq!(
            control_wire_command(PlaybackCommand::Stop {
                operation_id: OperationId::new(),
            })
            .expect("stop control"),
            WireCommand::Stop
        );
        assert_eq!(
            control_wire_command(PlaybackCommand::SetVolume {
                volume: 0.375_f32,
                operation_id: OperationId::new(),
            })
            .expect("volume control"),
            WireCommand::SetVolume { volume: 0.375_f64 }
        );

        for volume in [f32::NAN, f32::INFINITY, -0.01, 1.01] {
            let error = control_wire_command(PlaybackCommand::SetVolume {
                volume,
                operation_id: OperationId::new(),
            })
            .expect_err("invalid volume must not reach the transport");
            assert_eq!(error.user_message, "invalid_playback_volume");
        }
    }
}
