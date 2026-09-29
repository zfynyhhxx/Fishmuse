import { invoke } from "@tauri-apps/api/core";
import { listen, type Event, type UnlistenFn } from "@tauri-apps/api/event";

import {
  AI_APPLICATION_CONTRACT_VERSION,
  type AIEventEnvelope,
  type AppStatus,
  type PlaybackCommand,
  type PlaybackSnapshot,
  type SearchQuery,
  type StartTurnRequest,
  type TurnStarted,
} from "../contracts";

export const SCAN_PROGRESS_EVENT = "fishmuse://scan-progress";
export const AI_EVENT = "fishmuse://ai-event";
export const PLAYBACK_STATE_EVENT = "fishmuse://playback-state";
export const SERVICE_STATE_EVENT = "fishmuse://service-state";

const AI_EVENT_TYPES = new Set([
  "turn_started",
  "text_delta",
  "tool_started",
  "tool_finished",
  "usage",
  "turn_completed",
  "turn_failed",
]);
const UUID_V7 = /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

export function parseAIEventEnvelope(value: unknown): AIEventEnvelope {
  if (!value || typeof value !== "object") {
    throw new Error("Invalid AI event envelope");
  }
  const event = value as Record<string, unknown>;
  if (
    event.contract_version !== AI_APPLICATION_CONTRACT_VERSION ||
    typeof event.turn_id !== "string" ||
    !UUID_V7.test(event.turn_id) ||
    !Number.isSafeInteger(event.sequence) ||
    (event.sequence as number) < 1 ||
    typeof event.event_type !== "string" ||
    !AI_EVENT_TYPES.has(event.event_type)
  ) {
    throw new Error("Unsupported AI event envelope");
  }
  return value as AIEventEnvelope;
}

export async function startAITurn(
  request: StartTurnRequest,
  onEvent: (event: AIEventEnvelope) => void,
): Promise<{ turn: TurnStarted; unlisten: UnlistenFn }> {
  const unlisten = await listen<unknown>(AI_EVENT, (event: Event<unknown>) => {
    onEvent(parseAIEventEnvelope(event.payload));
  });
  try {
    const turn = await invoke<TurnStarted>("start_ai_turn", { request });
    return { turn, unlisten };
  } catch (error) {
    unlisten();
    throw error;
  }
}

export const getAppStatus = () => invoke<AppStatus>("get_app_status");
export const chooseLibraryFolders = () =>
  invoke<string[]>("choose_library_folders");
export const startLibraryScan = (roots: string[]) =>
  invoke<{ scan_id: string }>("start_library_scan", { roots });
export const cancelLibraryScan = (scanId: string) =>
  invoke<void>("cancel_library_scan", { scanId });
export const searchLibrary = (query: SearchQuery) =>
  invoke<unknown[]>("search_library", { query });
export const getLibraryItem = (trackId: string) =>
  invoke<unknown | null>("get_library_item", { trackId });
export const cancelAITurn = (turnId: string) =>
  invoke<void>("cancel_ai_turn", { turnId });
export const executePlayback = (command: PlaybackCommand) =>
  invoke<PlaybackSnapshot>("execute_playback", { command });
export const getPlaybackState = () =>
  invoke<PlaybackSnapshot>("get_playback_state");
export const configureDeepSeekKey = (apiKey: string) =>
  invoke<void>("configure_deepseek_key", { apiKey: { api_key: apiKey } });
export const deleteDeepSeekKey = () =>
  invoke<void>("delete_deepseek_key");
export const getAISettings = () => invoke<unknown>("get_ai_settings");
