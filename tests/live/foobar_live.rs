#![cfg(windows)]

use std::{env, fs};

use serde::Deserialize;

#[derive(Deserialize)]
struct Evidence {
    handshake_and_commands: bool,
    ack_within_500_ms: bool,
    restart_and_snapshot: bool,
    operation_id_replay: bool,
    wrong_sid_denied: bool,
}

#[test]
#[ignore = "controls a real foobar2000 process; run scripts/test-live-foobar.ps1 explicitly"]
fn foobar_live_acceptance_evidence() {
    assert_eq!(
        env::var("FISHMUSE_LIVE_FOOBAR_APPROVAL").as_deref(),
        Ok("interactive-approved"),
        "run the guarded PowerShell live-test script"
    );
    let path = env::var_os("FISHMUSE_LIVE_FOOBAR_EVIDENCE").expect("script-owned evidence path");
    let evidence: Evidence = serde_json::from_slice(&fs::read(path).expect("read evidence"))
        .expect("valid evidence JSON");
    assert!(evidence.handshake_and_commands);
    assert!(evidence.ack_within_500_ms);
    assert!(evidence.restart_and_snapshot);
    assert!(evidence.operation_id_replay);
    assert!(evidence.wrong_sid_denied);
}
