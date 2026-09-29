pub mod ai;
pub mod library;
pub mod playback;
pub mod settings;

use std::sync::Arc;

use tauri::State;

use crate::state::{AppState, AppStatusDto};

#[tauri::command]
pub fn get_app_status(state: State<'_, Arc<AppState>>) -> AppStatusDto {
    state.status()
}

#[tauri::command]
pub async fn choose_library_folders() -> Vec<String> {
    rfd::AsyncFileDialog::new()
        .set_title("Choose FishMuse library folders")
        .pick_folders()
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|folder| folder.path().to_string_lossy().into_owned())
        .collect()
}
