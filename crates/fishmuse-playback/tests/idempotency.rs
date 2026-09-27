use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;
use fishmuse_domain::{AppResult, OperationId};
use fishmuse_playback::{
    MemoryOperationStore, PlaybackBackend, PlaybackBackendKind, PlaybackCommand, PlaybackEvent,
    PlaybackManager, PlaybackSnapshot, PlaybackStatus,
};
use tokio::sync::{Notify, broadcast};

struct FakePlaybackBackend {
    executions: AtomicUsize,
    started: Notify,
    release: Notify,
    block: bool,
    sender: broadcast::Sender<PlaybackEvent>,
}

impl FakePlaybackBackend {
    fn immediate() -> Arc<Self> {
        let (sender, _) = broadcast::channel(8);
        Arc::new(Self {
            executions: AtomicUsize::new(0),
            started: Notify::new(),
            release: Notify::new(),
            block: false,
            sender,
        })
    }

    fn blocking() -> Arc<Self> {
        let (sender, _) = broadcast::channel(8);
        Arc::new(Self {
            executions: AtomicUsize::new(0),
            started: Notify::new(),
            release: Notify::new(),
            block: true,
            sender,
        })
    }
}

#[async_trait]
impl PlaybackBackend for FakePlaybackBackend {
    async fn execute(&self, _command: PlaybackCommand) -> AppResult<PlaybackSnapshot> {
        self.executions.fetch_add(1, Ordering::SeqCst);
        self.started.notify_one();
        if self.block {
            self.release.notified().await;
        }
        Ok(PlaybackSnapshot {
            revision: 1,
            status: PlaybackStatus::Paused,
            track_id: Some(fishmuse_domain::TrackId::new()),
            position_ms: 0,
            duration_ms: Some(1_000),
            backend: PlaybackBackendKind::Foobar2000,
        })
    }

    async fn snapshot(&self) -> AppResult<PlaybackSnapshot> {
        unreachable!("manager execution does not poll snapshots")
    }

    fn subscribe(&self) -> broadcast::Receiver<PlaybackEvent> {
        self.sender.subscribe()
    }
}

fn pause(operation_id: OperationId) -> PlaybackCommand {
    PlaybackCommand::Pause { operation_id }
}

#[tokio::test]
async fn sequential_retry_returns_saved_result_without_backend_reexecution() {
    let backend = FakePlaybackBackend::immediate();
    let manager = PlaybackManager::new(backend.clone(), Arc::new(MemoryOperationStore::new()));
    let operation_id = OperationId::new();

    let first = manager.execute(pause(operation_id)).await.expect("first");
    let retry = manager.execute(pause(operation_id)).await.expect("retry");

    assert_eq!(retry, first);
    assert_eq!(backend.executions.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn same_operation_id_with_a_different_command_conflicts() {
    let backend = FakePlaybackBackend::immediate();
    let manager = PlaybackManager::new(backend.clone(), Arc::new(MemoryOperationStore::new()));
    let operation_id = OperationId::new();
    manager.execute(pause(operation_id)).await.expect("first");

    let error = manager
        .execute(PlaybackCommand::Seek {
            position_ms: 10,
            operation_id,
        })
        .await
        .expect_err("different command must conflict");
    assert_eq!(error.user_message, "operation_conflict");
    assert_eq!(backend.executions.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn concurrent_duplicates_share_one_leader_execution() {
    let backend = FakePlaybackBackend::blocking();
    let manager = PlaybackManager::new(backend.clone(), Arc::new(MemoryOperationStore::new()));
    let operation_id = OperationId::new();

    let first = tokio::spawn({
        let manager = manager.clone();
        async move { manager.execute(pause(operation_id)).await }
    });
    backend.started.notified().await;
    let second = tokio::spawn({
        let manager = manager.clone();
        async move { manager.execute(pause(operation_id)).await }
    });
    tokio::task::yield_now().await;
    assert_eq!(backend.executions.load(Ordering::SeqCst), 1);
    backend.release.notify_waiters();

    assert_eq!(
        first.await.expect("first task").expect("first result"),
        second.await.expect("second task").expect("second result")
    );
    assert_eq!(backend.executions.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn caller_timeout_does_not_cancel_persistence_and_retry_reads_result() {
    let backend = FakePlaybackBackend::blocking();
    let manager = PlaybackManager::new(backend.clone(), Arc::new(MemoryOperationStore::new()));
    let operation_id = OperationId::new();

    let timed_out = tokio::spawn({
        let manager = manager.clone();
        async move {
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                manager.execute(pause(operation_id)),
            )
            .await
        }
    });
    backend.started.notified().await;
    tokio::time::advance(std::time::Duration::from_secs(10)).await;
    let timed_out = timed_out.await.expect("caller task");
    assert!(timed_out.is_err());
    assert_eq!(backend.executions.load(Ordering::SeqCst), 1);

    backend.release.notify_waiters();
    let retry = manager
        .execute(pause(operation_id))
        .await
        .expect("saved result");
    assert_eq!(retry.status, PlaybackStatus::Paused);
    assert_eq!(backend.executions.load(Ordering::SeqCst), 1);
}
