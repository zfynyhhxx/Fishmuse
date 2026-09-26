use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceState {
    Unavailable,
    NotConfigured,
}

#[derive(Debug, Serialize)]
pub struct HealthStatus {
    pub version: String,
    pub database: ServiceState,
    pub playback: ServiceState,
    pub ai: ServiceState,
}

#[tauri::command]
fn healthcheck() -> HealthStatus {
    HealthStatus {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        database: ServiceState::Unavailable,
        playback: ServiceState::Unavailable,
        ai: ServiceState::NotConfigured,
    }
}

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![healthcheck])
        .run(tauri::generate_context!())
        .expect("error while running FishMuse desktop application");
}
