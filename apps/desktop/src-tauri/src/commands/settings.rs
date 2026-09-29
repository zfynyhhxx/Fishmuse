use std::sync::Arc;

use tauri::State;

use crate::{
    error::CommandError,
    state::{AISettingsDto, AppState, SecretStringDto},
};

#[tauri::command]
pub async fn configure_deepseek_key(
    state: State<'_, Arc<AppState>>,
    api_key: SecretStringDto,
) -> Result<(), CommandError> {
    state.configure_ai_key(api_key).await
}

#[tauri::command]
pub async fn delete_deepseek_key(state: State<'_, Arc<AppState>>) -> Result<(), CommandError> {
    state.delete_ai_key().await
}

#[tauri::command]
pub async fn get_ai_settings(
    state: State<'_, Arc<AppState>>,
) -> Result<AISettingsDto, CommandError> {
    state.ai_settings().await
}
