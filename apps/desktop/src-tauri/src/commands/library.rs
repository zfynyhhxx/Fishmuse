use std::sync::Arc;

use fishmuse_domain::{LibraryItem, ScanId, TrackId, TrackSummary};
use tauri::State;

use crate::{
    error::CommandError,
    state::{AppState, ScanStartedDto, SearchQueryDto},
};

#[tauri::command]
pub async fn start_library_scan(
    state: State<'_, Arc<AppState>>,
    roots: Vec<String>,
) -> Result<ScanStartedDto, CommandError> {
    state.inner().start_scan(roots).await
}

#[tauri::command]
pub async fn cancel_library_scan(
    state: State<'_, Arc<AppState>>,
    scan_id: ScanId,
) -> Result<(), CommandError> {
    state.cancel_scan(scan_id).await
}

#[tauri::command]
pub async fn search_library(
    state: State<'_, Arc<AppState>>,
    query: SearchQueryDto,
) -> Result<Vec<TrackSummary>, CommandError> {
    state.search_library(query).await
}

#[tauri::command]
pub async fn get_library_item(
    state: State<'_, Arc<AppState>>,
    track_id: TrackId,
) -> Result<Option<LibraryItem>, CommandError> {
    state.get_library_item(track_id).await
}
