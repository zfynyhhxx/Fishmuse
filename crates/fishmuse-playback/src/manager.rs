use std::{collections::HashMap, sync::Arc};

use fishmuse_domain::{AppError, AppResult, ErrorCategory, ErrorCode, OperationId};
use tokio::sync::{Mutex, watch};

use crate::{
    CommandFingerprint, OperationClaim, OperationStore, PlaybackBackend, PlaybackCommand,
    PlaybackSnapshot,
};

pub struct PlaybackManager {
    backend: Arc<dyn PlaybackBackend>,
    operations: Arc<dyn OperationStore>,
    in_flight: Arc<Mutex<HashMap<OperationId, Arc<InFlight>>>>,
}

impl Clone for PlaybackManager {
    fn clone(&self) -> Self {
        Self {
            backend: self.backend.clone(),
            operations: self.operations.clone(),
            in_flight: self.in_flight.clone(),
        }
    }
}

struct InFlight {
    fingerprint: CommandFingerprint,
    outcome: watch::Sender<Option<AppResult<PlaybackSnapshot>>>,
}

impl PlaybackManager {
    #[must_use]
    pub fn new<B, S>(backend: Arc<B>, operations: Arc<S>) -> Self
    where
        B: PlaybackBackend + 'static,
        S: OperationStore + 'static,
    {
        Self {
            backend,
            operations,
            in_flight: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn execute(&self, command: PlaybackCommand) -> AppResult<PlaybackSnapshot> {
        let operation_id = command.operation_id();
        let fingerprint = command.fingerprint();
        let (entry, leader) = {
            let mut in_flight = self.in_flight.lock().await;
            if let Some(entry) = in_flight.get(&operation_id) {
                if entry.fingerprint != fingerprint {
                    return Err(operation_conflict());
                }
                (entry.clone(), false)
            } else {
                let (outcome, _) = watch::channel(None);
                let entry = Arc::new(InFlight {
                    fingerprint,
                    outcome,
                });
                in_flight.insert(operation_id, entry.clone());
                (entry, true)
            }
        };

        if leader {
            match self.operations.try_begin(operation_id, fingerprint).await {
                Ok(OperationClaim::Acquired) => self.spawn_leader(operation_id, command, &entry),
                Ok(OperationClaim::Completed(snapshot)) => {
                    self.publish(operation_id, &entry, Ok(snapshot)).await;
                }
                Ok(OperationClaim::Conflict) => {
                    self.publish(operation_id, &entry, Err(operation_conflict()))
                        .await;
                }
                Ok(OperationClaim::InFlight) => {
                    self.publish(operation_id, &entry, Err(operation_in_flight()))
                        .await;
                }
                Err(error) => self.publish(operation_id, &entry, Err(error)).await,
            }
        }

        wait_for_outcome(&entry).await
    }

    fn spawn_leader(
        &self,
        operation_id: OperationId,
        command: PlaybackCommand,
        entry: &Arc<InFlight>,
    ) {
        let backend = self.backend.clone();
        let operations = self.operations.clone();
        let in_flight = self.in_flight.clone();
        let entry = entry.clone();
        tokio::spawn(async move {
            let backend_result = backend.execute(command).await;
            let outcome = match operations.complete(operation_id, &backend_result).await {
                Ok(()) => backend_result,
                Err(error) => Err(error),
            };
            entry.outcome.send_replace(Some(outcome));
            in_flight.lock().await.remove(&operation_id);
        });
    }

    async fn publish(
        &self,
        operation_id: OperationId,
        entry: &InFlight,
        result: AppResult<PlaybackSnapshot>,
    ) {
        entry.outcome.send_replace(Some(result));
        self.in_flight.lock().await.remove(&operation_id);
    }
}

async fn wait_for_outcome(entry: &InFlight) -> AppResult<PlaybackSnapshot> {
    let mut receiver = entry.outcome.subscribe();
    loop {
        if let Some(result) = receiver.borrow().clone() {
            return result;
        }
        receiver
            .changed()
            .await
            .map_err(|_| operation_in_flight())?;
    }
}

fn operation_conflict() -> AppError {
    AppError {
        code: ErrorCode::InvalidInput,
        category: ErrorCategory::Playback,
        user_message: "operation_conflict".to_owned(),
        retryable: false,
        suggested_action: None,
        technical_context: None,
    }
}

fn operation_in_flight() -> AppError {
    AppError {
        code: ErrorCode::BackendUnavailable,
        category: ErrorCategory::Playback,
        user_message: "operation_in_flight".to_owned(),
        retryable: true,
        suggested_action: Some("retry_later".to_owned()),
        technical_context: None,
    }
}
