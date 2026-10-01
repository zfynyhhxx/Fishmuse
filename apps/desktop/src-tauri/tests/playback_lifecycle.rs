use std::{
    collections::HashSet,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use fishmuse_desktop::playback_lifecycle::{
    LaunchOutcome, ManagedPlaybackService, PlaybackBackendLauncher, PlaybackConnection,
    PlaybackConnectionState,
};
use fishmuse_domain::{AppError, AppResult, ErrorCategory, ErrorCode, OperationId};
use fishmuse_playback::{
    PlaybackBackendKind, PlaybackCommand, PlaybackControl, PlaybackEvent, PlaybackServiceStatus,
    PlaybackSnapshot, PlaybackStatus,
};
use tokio::sync::{Notify, broadcast, watch};

struct RecordingControl {
    operation_ids: Mutex<Vec<OperationId>>,
    executions: AtomicUsize,
    fail_after_accept: AtomicBool,
    events: broadcast::Sender<PlaybackEvent>,
}

impl RecordingControl {
    fn new() -> Arc<Self> {
        let (events, _) = broadcast::channel(8);
        Arc::new(Self {
            operation_ids: Mutex::new(Vec::new()),
            executions: AtomicUsize::new(0),
            fail_after_accept: AtomicBool::new(false),
            events,
        })
    }

    fn snapshot() -> PlaybackSnapshot {
        PlaybackSnapshot {
            revision: 1,
            status: PlaybackStatus::Stopped,
            track_id: None,
            position_ms: 0,
            duration_ms: None,
            volume: 1.0,
            backend: PlaybackBackendKind::Foobar2000,
        }
    }
}

#[async_trait]
impl PlaybackControl for RecordingControl {
    async fn execute(&self, command: PlaybackCommand) -> AppResult<PlaybackSnapshot> {
        self.executions.fetch_add(1, Ordering::SeqCst);
        self.operation_ids
            .lock()
            .expect("operation IDs")
            .push(command.operation_id());
        if self.fail_after_accept.load(Ordering::SeqCst) {
            return Err(AppError {
                code: ErrorCode::BackendUnavailable,
                category: ErrorCategory::Playback,
                user_message: "The playback command outcome is unknown.".to_owned(),
                retryable: true,
                suggested_action: Some("refresh_playback_state".to_owned()),
                technical_context: Some("ack_lost_after_write".to_owned()),
            });
        }
        Ok(Self::snapshot())
    }

    async fn snapshot(&self) -> AppResult<PlaybackSnapshot> {
        Ok(Self::snapshot())
    }

    fn subscribe(&self) -> broadcast::Receiver<PlaybackEvent> {
        self.events.subscribe()
    }
}

struct FakeLauncher {
    launches: AtomicUsize,
    fail: AtomicBool,
    launched: Notify,
}

impl FakeLauncher {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            launches: AtomicUsize::new(0),
            fail: AtomicBool::new(false),
            launched: Notify::new(),
        })
    }
}

impl PlaybackBackendLauncher for FakeLauncher {
    fn launch_hidden(&self) -> AppResult<LaunchOutcome> {
        self.launches.fetch_add(1, Ordering::SeqCst);
        self.launched.notify_waiters();
        if self.fail.load(Ordering::SeqCst) {
            return Err(AppError {
                code: ErrorCode::BackendUnavailable,
                category: ErrorCategory::Playback,
                user_message: "The playback service could not be started.".to_owned(),
                retryable: true,
                suggested_action: Some("open_advanced_playback_diagnostics".to_owned()),
                technical_context: Some("launcher failed".to_owned()),
            });
        }
        Ok(LaunchOutcome {
            process_id: Some(42),
        })
    }
}

struct FakeConnection {
    state: watch::Sender<PlaybackConnectionState>,
    reconnects: AtomicUsize,
    ready_after_reconnects: AtomicUsize,
    shutdown: AtomicBool,
}

impl FakeConnection {
    fn new(initial: PlaybackConnectionState) -> Arc<Self> {
        let (state, _) = watch::channel(initial);
        Arc::new(Self {
            state,
            reconnects: AtomicUsize::new(0),
            ready_after_reconnects: AtomicUsize::new(0),
            shutdown: AtomicBool::new(false),
        })
    }

    fn set(&self, state: PlaybackConnectionState) {
        self.state.send_replace(state);
    }

    fn become_ready_after(&self, reconnects: usize) {
        self.ready_after_reconnects
            .store(reconnects, Ordering::SeqCst);
    }
}

#[async_trait]
impl PlaybackConnection for FakeConnection {
    fn subscribe(&self) -> watch::Receiver<PlaybackConnectionState> {
        self.state.subscribe()
    }

    fn reconnect_now(&self) {
        let reconnects = self.reconnects.fetch_add(1, Ordering::SeqCst) + 1;
        let threshold = self.ready_after_reconnects.load(Ordering::SeqCst);
        if threshold > 0 && reconnects >= threshold {
            self.state.send_replace(PlaybackConnectionState::Ready);
        }
    }

    async fn shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
    }
}

fn pause(operation_id: OperationId) -> PlaybackCommand {
    PlaybackCommand::Pause { operation_id }
}

fn service(
    control: Arc<RecordingControl>,
    launcher: Arc<FakeLauncher>,
    connection: Arc<FakeConnection>,
) -> Arc<ManagedPlaybackService> {
    Arc::new(ManagedPlaybackService::new(
        control,
        launcher,
        connection,
        Duration::from_secs(5),
    ))
}

async fn wait_for_launch(launcher: &FakeLauncher) {
    while launcher.launches.load(Ordering::SeqCst) == 0 {
        launcher.launched.notified().await;
    }
}

#[tokio::test]
async fn ready_service_executes_without_launching() {
    let control = RecordingControl::new();
    let launcher = FakeLauncher::new();
    let connection = FakeConnection::new(PlaybackConnectionState::Ready);
    let service = service(control.clone(), launcher.clone(), connection.clone());

    let operation_id = OperationId::new();
    service.execute(pause(operation_id)).await.expect("execute");

    assert_eq!(launcher.launches.load(Ordering::SeqCst), 0);
    assert_eq!(connection.reconnects.load(Ordering::SeqCst), 0);
    assert_eq!(
        control.operation_ids.lock().unwrap().as_slice(),
        &[operation_id]
    );
}

#[tokio::test]
async fn disconnected_service_publishes_starting_then_ready_before_execution() {
    let control = RecordingControl::new();
    let launcher = FakeLauncher::new();
    let connection = FakeConnection::new(PlaybackConnectionState::Unavailable);
    let service = service(control.clone(), launcher.clone(), connection.clone());
    let mut states = service.subscribe_service_state();

    let operation_id = OperationId::new();
    let execute = tokio::spawn({
        let service = service.clone();
        async move { service.execute(pause(operation_id)).await }
    });
    wait_for_launch(&launcher).await;
    states.changed().await.expect("starting state");
    assert_eq!(states.borrow().status, PlaybackServiceStatus::Starting);
    assert_eq!(control.executions.load(Ordering::SeqCst), 0);

    connection.set(PlaybackConnectionState::Ready);
    execute.await.expect("task").expect("command");
    states.changed().await.expect("ready state");
    assert_eq!(states.borrow().status, PlaybackServiceStatus::Ready);
    assert_eq!(
        control.operation_ids.lock().unwrap().as_slice(),
        &[operation_id]
    );
}

#[tokio::test(start_paused = true)]
async fn startup_reconnect_pulses_close_the_launch_to_pipe_readiness_race() {
    let control = RecordingControl::new();
    let launcher = FakeLauncher::new();
    let connection = FakeConnection::new(PlaybackConnectionState::Unavailable);
    connection.become_ready_after(3);
    let service = service(control.clone(), launcher.clone(), connection.clone());

    let execute = tokio::spawn({
        let service = service.clone();
        async move { service.execute(pause(OperationId::new())).await }
    });
    wait_for_launch(&launcher).await;
    tokio::time::advance(Duration::from_millis(250)).await;
    execute.await.expect("task").expect("command");

    assert_eq!(launcher.launches.load(Ordering::SeqCst), 1);
    assert_eq!(connection.reconnects.load(Ordering::SeqCst), 3);
    assert_eq!(control.executions.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn readiness_wait_times_out_after_five_seconds() {
    let control = RecordingControl::new();
    let launcher = FakeLauncher::new();
    let connection = FakeConnection::new(PlaybackConnectionState::Unavailable);
    let service = service(control.clone(), launcher.clone(), connection);

    let execute = tokio::spawn({
        let service = service.clone();
        async move { service.execute(pause(OperationId::new())).await }
    });
    wait_for_launch(&launcher).await;
    tokio::time::advance(Duration::from_secs(5)).await;
    let error = execute
        .await
        .expect("task")
        .expect_err("connection timeout");

    assert_eq!(error.code, ErrorCode::Unavailable);
    assert_eq!(
        error.user_message,
        "The playback service did not become ready in time."
    );
    assert_eq!(control.executions.load(Ordering::SeqCst), 0);
    assert_eq!(
        service.service_state().status,
        PlaybackServiceStatus::Unavailable
    );
}

#[tokio::test]
async fn launch_failure_is_generic_and_does_not_execute_the_command() {
    let control = RecordingControl::new();
    let launcher = FakeLauncher::new();
    launcher.fail.store(true, Ordering::SeqCst);
    let connection = FakeConnection::new(PlaybackConnectionState::Unavailable);
    let service = service(control.clone(), launcher, connection);

    let error = service
        .execute(pause(OperationId::new()))
        .await
        .expect_err("launch failure");

    assert!(
        !serde_json::to_string(&error)
            .unwrap()
            .to_ascii_lowercase()
            .contains("foobar")
    );
    assert_eq!(control.executions.load(Ordering::SeqCst), 0);
    assert_eq!(
        service.service_state().status,
        PlaybackServiceStatus::Unavailable
    );
}

#[tokio::test]
async fn ten_concurrent_commands_share_one_launch_and_preserve_every_operation_id() {
    let control = RecordingControl::new();
    let launcher = FakeLauncher::new();
    let connection = FakeConnection::new(PlaybackConnectionState::Unavailable);
    let service = service(control.clone(), launcher.clone(), connection.clone());
    let operation_ids: Vec<_> = (0..10).map(|_| OperationId::new()).collect();

    let tasks: Vec<_> = operation_ids
        .iter()
        .copied()
        .map(|operation_id| {
            let service = service.clone();
            tokio::spawn(async move { service.execute(pause(operation_id)).await })
        })
        .collect();
    wait_for_launch(&launcher).await;
    tokio::task::yield_now().await;
    assert_eq!(launcher.launches.load(Ordering::SeqCst), 1);
    connection.set(PlaybackConnectionState::Ready);
    for task in tasks {
        task.await.expect("task").expect("command");
    }

    let actual: HashSet<_> = control
        .operation_ids
        .lock()
        .unwrap()
        .iter()
        .copied()
        .collect();
    let expected: HashSet<_> = operation_ids.into_iter().collect();
    assert_eq!(actual, expected);
    assert_eq!(control.executions.load(Ordering::SeqCst), 10);
    assert_eq!(launcher.launches.load(Ordering::SeqCst), 1);
    assert_eq!(connection.reconnects.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn ambiguous_command_failure_is_never_reissued() {
    let control = RecordingControl::new();
    control.fail_after_accept.store(true, Ordering::SeqCst);
    let launcher = FakeLauncher::new();
    let connection = FakeConnection::new(PlaybackConnectionState::Unavailable);
    let service = service(control.clone(), launcher.clone(), connection.clone());

    let execute = tokio::spawn({
        let service = service.clone();
        async move { service.execute(pause(OperationId::new())).await }
    });
    wait_for_launch(&launcher).await;
    connection.set(PlaybackConnectionState::Ready);
    execute.await.expect("task").expect_err("ambiguous result");

    assert_eq!(launcher.launches.load(Ordering::SeqCst), 1);
    assert_eq!(control.executions.load(Ordering::SeqCst), 1);
}
