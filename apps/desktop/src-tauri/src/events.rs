use std::sync::Mutex;

use fishmuse_ai::AIEventEnvelope;
use fishmuse_domain::{AppError, AppResult, ErrorCategory, ErrorCode};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::state::{PlaybackSnapshotDto, ScanProgressDto, ServiceStateEventDto};

pub const SCAN_PROGRESS_EVENT: &str = "fishmuse://scan-progress";
pub const AI_EVENT: &str = "fishmuse://ai-event";
pub const PLAYBACK_STATE_EVENT: &str = "fishmuse://playback-state";
pub const SERVICE_STATE_EVENT: &str = "fishmuse://service-state";

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "channel", content = "payload", rename_all = "snake_case")]
pub enum ApplicationEvent {
    ScanProgress(ScanProgressDto),
    Ai(AIEventEnvelope),
    PlaybackState(PlaybackSnapshotDto),
    ServiceState(ServiceStateEventDto),
}

pub trait ApplicationEventSink: Send + Sync {
    fn emit(&self, event: ApplicationEvent) -> AppResult<()>;
}

pub struct TauriEventSink {
    app: AppHandle,
}

impl TauriEventSink {
    #[must_use]
    pub const fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl ApplicationEventSink for TauriEventSink {
    fn emit(&self, event: ApplicationEvent) -> AppResult<()> {
        let result = match event {
            ApplicationEvent::ScanProgress(payload) => self.app.emit(SCAN_PROGRESS_EVENT, payload),
            ApplicationEvent::Ai(payload) => self.app.emit(AI_EVENT, payload),
            ApplicationEvent::PlaybackState(payload) => {
                self.app.emit(PLAYBACK_STATE_EVENT, payload)
            }
            ApplicationEvent::ServiceState(payload) => self.app.emit(SERVICE_STATE_EVENT, payload),
        };
        result.map_err(|error| AppError {
            code: ErrorCode::Internal,
            category: ErrorCategory::Internal,
            user_message: "The application event could not be delivered.".to_owned(),
            retryable: true,
            suggested_action: None,
            technical_context: Some(error.to_string()),
        })
    }
}

#[derive(Default)]
pub struct MemoryEventSink {
    events: Mutex<Vec<ApplicationEvent>>,
}

impl MemoryEventSink {
    pub fn events(&self) -> Vec<ApplicationEvent> {
        self.events
            .lock()
            .expect("event sink lock poisoned")
            .clone()
    }
}

impl ApplicationEventSink for MemoryEventSink {
    fn emit(&self, event: ApplicationEvent) -> AppResult<()> {
        self.events
            .lock()
            .expect("event sink lock poisoned")
            .push(event);
        Ok(())
    }
}
