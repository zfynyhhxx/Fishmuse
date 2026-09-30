use std::sync::Arc;

use tauri::State;

use crate::{
    error::CommandError,
    state::{AppState, PlaybackCommandDto, PlaybackSnapshotDto, QueueCommandDto, QueueSnapshotDto},
};

#[tauri::command]
pub async fn execute_playback(
    state: State<'_, Arc<AppState>>,
    command: PlaybackCommandDto,
) -> Result<PlaybackSnapshotDto, CommandError> {
    state.execute_playback(command).await
}

#[tauri::command]
pub async fn get_playback_state(
    state: State<'_, Arc<AppState>>,
) -> Result<PlaybackSnapshotDto, CommandError> {
    state.playback_snapshot().await
}

#[tauri::command]
pub async fn execute_queue_command(
    state: State<'_, Arc<AppState>>,
    command: QueueCommandDto,
) -> Result<QueueSnapshotDto, CommandError> {
    state.execute_queue_command(command).await
}

#[tauri::command]
pub async fn get_playback_queue(
    state: State<'_, Arc<AppState>>,
) -> Result<QueueSnapshotDto, CommandError> {
    state.playback_queue().await
}

#[tauri::command]
pub async fn retry_playback_service(state: State<'_, Arc<AppState>>) -> Result<(), CommandError> {
    state.retry_playback_service().await
}
