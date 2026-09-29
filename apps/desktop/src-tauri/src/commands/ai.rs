use std::sync::Arc;

use fishmuse_ai::AITurnId;
use tauri::State;

use crate::{
    error::CommandError,
    state::{AppState, StartTurnDto, TurnStartedDto},
};

#[tauri::command]
pub async fn start_ai_turn(
    state: State<'_, Arc<AppState>>,
    request: StartTurnDto,
) -> Result<TurnStartedDto, CommandError> {
    state.inner().start_ai_turn(request).await
}

#[tauri::command]
pub async fn cancel_ai_turn(
    state: State<'_, Arc<AppState>>,
    turn_id: AITurnId,
) -> Result<(), CommandError> {
    state.cancel_ai_turn(turn_id).await
}
