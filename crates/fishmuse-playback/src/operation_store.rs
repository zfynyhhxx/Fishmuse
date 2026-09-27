use std::{collections::HashMap, sync::Mutex};

use async_trait::async_trait;
use fishmuse_domain::{AppResult, OperationId};

use crate::{CommandFingerprint, PlaybackSnapshot};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OperationClaim {
    Acquired,
    InFlight,
    Completed(PlaybackSnapshot),
    Conflict,
}

#[async_trait]
pub trait OperationStore: Send + Sync {
    async fn try_begin(
        &self,
        operation_id: OperationId,
        command_fingerprint: CommandFingerprint,
    ) -> AppResult<OperationClaim>;

    async fn complete(
        &self,
        operation_id: OperationId,
        result: &AppResult<PlaybackSnapshot>,
    ) -> AppResult<()>;
}

#[derive(Default)]
pub struct MemoryOperationStore {
    records: Mutex<HashMap<OperationId, MemoryRecord>>,
}

impl MemoryOperationStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

enum MemoryRecord {
    Pending(CommandFingerprint),
    Completed(CommandFingerprint, AppResult<PlaybackSnapshot>),
}

#[async_trait]
impl OperationStore for MemoryOperationStore {
    async fn try_begin(
        &self,
        operation_id: OperationId,
        command_fingerprint: CommandFingerprint,
    ) -> AppResult<OperationClaim> {
        let mut records = self.records.lock().expect("operation store lock poisoned");
        match records.get(&operation_id) {
            None => {
                records.insert(operation_id, MemoryRecord::Pending(command_fingerprint));
                Ok(OperationClaim::Acquired)
            }
            Some(MemoryRecord::Pending(fingerprint)) if *fingerprint == command_fingerprint => {
                Ok(OperationClaim::InFlight)
            }
            Some(MemoryRecord::Completed(fingerprint, result))
                if *fingerprint == command_fingerprint =>
            {
                match result {
                    Ok(snapshot) => Ok(OperationClaim::Completed(snapshot.clone())),
                    Err(error) => Err(error.clone()),
                }
            }
            Some(_) => Ok(OperationClaim::Conflict),
        }
    }

    async fn complete(
        &self,
        operation_id: OperationId,
        result: &AppResult<PlaybackSnapshot>,
    ) -> AppResult<()> {
        let mut records = self.records.lock().expect("operation store lock poisoned");
        if let Some(MemoryRecord::Pending(fingerprint)) = records.get(&operation_id) {
            let fingerprint = *fingerprint;
            records.insert(
                operation_id,
                MemoryRecord::Completed(fingerprint, result.clone()),
            );
        }
        Ok(())
    }
}
