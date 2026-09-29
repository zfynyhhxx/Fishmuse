use std::sync::Arc;

use tauri::State;

use crate::{
    error::CommandError,
    state::{AppState, PlaybackCommandDto, PlaybackSnapshotDto},
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
