import { browser } from "@wdio/globals";

export const TRACK_ID = "018f0000-0000-7000-8000-000000000001";
export const TURN_ID = "018f0000-0000-7000-8000-000000000002";
export const SCAN_ID = "018f0000-0000-7000-8000-000000000003";

const track = {
  id: TRACK_ID,
  recording_id: "018f0000-0000-7000-8000-000000000004",
  title: "So What",
  artist_names: ["Miles Davis"],
  release_title: "Kind of Blue",
  duration_ms: 545_000,
  disc_number: 1,
  track_number: 1,
  playable: true,
};

export async function installDeterministicFakes() {
  await browser.execute((commands) => {
    localStorage.clear();
    window.__FISHMUSE_E2E_COMMANDS__ = commands;
    window.__FISHMUSE_E2E_CALLS__ = {};
    window.location.hash = "#/onboarding";
    window.dispatchEvent(new HashChangeEvent("hashchange"));
  }, {
    get_app_status: {
      version: "0.1.0",
      database: "ready",
      playback: { status: "ready", implementation: { id: "fake", display_name: "Fake Playback" } },
      ai: { status: "ready", implementation: { id: "fake", display_name: "Fake AI" } },
    },
    get_playback_state: {
      revision: 0,
      status: "stopped",
      track_id: null,
      position_ms: 0,
      duration_ms: null,
    },
    get_ai_settings: {
      configured: true,
      provider: "Fake AI",
      model: "deterministic",
      service: { status: "ready", implementation: { id: "fake", display_name: "Fake AI" } },
      budget: {
        spent_microunits: 0,
        warning_at_microunits: 10_000_000,
        hard_stop_at_microunits: 20_000_000,
      },
    },
    choose_library_folders: ["C:\\FishMuseE2E\\Music"],
    start_library_scan: { scan_id: SCAN_ID },
    search_library: [track],
    execute_playback: {
      revision: 1,
      status: "playing",
      track_id: TRACK_ID,
      position_ms: 0,
      duration_ms: 545_000,
    },
    start_ai_turn: { turn_id: TURN_ID },
    cancel_ai_turn: null,
  });
}

export async function waitForCommand(command: string) {
  await browser.waitUntil(() => browser.execute(
    (commandName) => (window.__FISHMUSE_E2E_CALLS__?.[commandName]?.length ?? 0) > 0,
    command,
  ));
}

export async function completeOnboarding() {
  await browser.execute(() => {
    localStorage.setItem("fishmuse.onboarding.complete", "true");
    window.location.hash = "#/library";
  });
}

export async function navigate(hash: string) {
  await browser.execute((nextHash) => {
    window.location.hash = nextHash;
  }, hash);
}

export async function emitEvent(name: string, payload: unknown) {
  await browser.tauri.execute(
    async (_tauri, eventName, eventPayload) => {
      await window.__TAURI__.event.emit(eventName, eventPayload);
    },
    name,
    payload,
  );
}

export async function emitAI(sequence: number, event: Record<string, unknown>) {
  await emitEvent("fishmuse://ai-event", {
    contract_version: 1,
    turn_id: TURN_ID,
    sequence,
    ...event,
  });
}
