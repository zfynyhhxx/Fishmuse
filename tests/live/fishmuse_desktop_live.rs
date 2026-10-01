#![cfg(windows)]

use std::{env, fs, path::PathBuf};

use serde::Deserialize;
use sqlx::{sqlite::SqliteConnectOptions, sqlite::SqlitePoolOptions};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Evidence {
    backend_initially_stopped: bool,
    fishmuse_play_started_backend: bool,
    no_visible_backend_window: bool,
    backend_never_foreground: bool,
    playback_reached_playing: bool,
    pause_resume: bool,
    seek: bool,
    volume: bool,
    mute_unmute: bool,
    next_previous: bool,
    stop: bool,
    listening_history_persisted: bool,
    fishmuse_shutdown_clean: bool,
    ui_only_playback_commands: bool,
}

impl Evidence {
    fn validate(&self) -> Result<(), &'static str> {
        let required = [
            self.backend_initially_stopped,
            self.fishmuse_play_started_backend,
            self.no_visible_backend_window,
            self.backend_never_foreground,
            self.playback_reached_playing,
            self.pause_resume,
            self.seek,
            self.volume,
            self.mute_unmute,
            self.next_previous,
            self.stop,
            self.listening_history_persisted,
            self.fishmuse_shutdown_clean,
            self.ui_only_playback_commands,
        ];
        required
            .into_iter()
            .all(|value| value)
            .then_some(())
            .ok_or("every FishMuse desktop live evidence field must be true")
    }
}

fn parse_evidence(bytes: &[u8]) -> Result<Evidence, String> {
    if bytes.len() > 4_096 {
        return Err("evidence exceeds the 4096-byte bound".to_owned());
    }
    let evidence: Evidence = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
    evidence.validate().map_err(str::to_owned)?;
    Ok(evidence)
}

#[test]
#[ignore = "starts the real FishMuse release-mode harness; run scripts/test-live-fishmuse.ps1 explicitly"]
fn fishmuse_desktop_live_acceptance_evidence() {
    assert_eq!(
        env::var("FISHMUSE_LIVE_DESKTOP_APPROVAL").as_deref(),
        Ok("interactive-approved"),
        "run the guarded PowerShell live-test script"
    );
    let evidence_path =
        PathBuf::from(env::var_os("FISHMUSE_LIVE_DESKTOP_EVIDENCE").expect("evidence path"));
    let database_path =
        PathBuf::from(env::var_os("FISHMUSE_LIVE_DESKTOP_DATABASE").expect("database path"));
    parse_evidence(&fs::read(&evidence_path).expect("read bounded evidence"))
        .expect("valid all-true evidence");

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Tokio runtime");
    let listen_count = runtime.block_on(async {
        let options = SqliteConnectOptions::new()
            .filename(&database_path)
            .read_only(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .expect("open test-owned FishMuse database");
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM listening_events WHERE ended_at IS NOT NULL",
        )
        .fetch_one(&pool)
        .await
        .expect("query persisted listening history")
    });
    assert!(
        listen_count > 0,
        "normal FishMuse playback must persist a settled listen"
    );
}

#[cfg(test)]
mod tests {
    use super::parse_evidence;

    const VALID: &str = r#"{"backend_initially_stopped":true,"fishmuse_play_started_backend":true,"no_visible_backend_window":true,"backend_never_foreground":true,"playback_reached_playing":true,"pause_resume":true,"seek":true,"volume":true,"mute_unmute":true,"next_previous":true,"stop":true,"listening_history_persisted":true,"fishmuse_shutdown_clean":true,"ui_only_playback_commands":true}"#;

    #[test]
    fn accepts_exact_all_true_bounded_evidence() {
        parse_evidence(VALID.as_bytes()).expect("valid evidence");
    }

    #[test]
    fn rejects_missing_false_and_unknown_evidence() {
        assert!(parse_evidence(VALID.replace(",\"stop\":true", "").as_bytes()).is_err());
        assert!(
            parse_evidence(VALID.replace("\"stop\":true", "\"stop\":false").as_bytes()).is_err()
        );
        assert!(parse_evidence(VALID.replace('}', ",\"unexpected\":true}").as_bytes()).is_err());
    }
}
