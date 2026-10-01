import { browser } from "@wdio/globals";

export const TRACK_ID = "018f0000-0000-7000-8000-000000000001";
export const SECOND_TRACK_ID = "018f0000-0000-7000-8000-000000000005";
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
      position_ms: 0,
      duration_ms: null,
      volume: 1,
      muted: false,
      track: null,
      queue: { track_ids: [], current_index: null, can_previous: false, can_next: false },
      external: false,
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
    cancel_library_scan: null,
    search_library: [track],
    execute_playback: {
      revision: 1,
      status: "playing",
      position_ms: 0,
      duration_ms: 545_000,
      volume: 1,
      muted: false,
      track: {
        id: TRACK_ID,
        title: "So What",
        artist_names: ["Miles Davis"],
        release_title: "Kind of Blue",
        artwork_available: false,
      },
      queue: { track_ids: [TRACK_ID], current_index: 0, can_previous: true, can_next: false },
      external: false,
    },
    execute_queue_command: {
      track_ids: [TRACK_ID],
      current_index: 0,
      can_previous: true,
      can_next: false,
    },
    get_track_artwork: null,
    retry_playback_service: null,
    start_ai_turn: { turn_id: TURN_ID },
    cancel_ai_turn: null,
  });
}

export async function waitForCommand(command: string) {
  await waitForCommandCount(command, 1);
}

export async function waitForCommandCount(command: string, count: number) {
  await browser.waitUntil(() => browser.execute(
    (commandName, minimum) => (window.__FISHMUSE_E2E_CALLS__?.[commandName]?.length ?? 0) >= minimum,
    command,
    count,
  ));
}

export async function commandCalls(command: string) {
  return browser.execute(
    (commandName) => window.__FISHMUSE_E2E_CALLS__?.[commandName] ?? [],
    command,
  );
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
