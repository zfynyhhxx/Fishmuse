import { invoke } from "@tauri-apps/api/core";
import { listen, type Event, type UnlistenFn } from "@tauri-apps/api/event";

import {
  AI_APPLICATION_CONTRACT_VERSION,
  type AIEventEnvelope,
  type AISettings,
  type AppStatus,
  type LibraryItem,
  type PlaybackCommand,
  type PlaybackSnapshot,
  type ScanProgress,
  type SearchQuery,
  type ServiceStateEvent,
  type StartTurnRequest,
  type TrackSummary,
  type TurnStarted,
} from "../contracts";

declare global {
  interface Window {
    __FISHMUSE_E2E_COMMANDS__?: Record<string, unknown>;
    __FISHMUSE_E2E_CALLS__?: Record<string, unknown[]>;
  }
}

function invokeCommand<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (import.meta.env.MODE === "e2e") {
    const commands = window.__FISHMUSE_E2E_COMMANDS__;
    if (!commands || !(command in commands)) {
      return Promise.reject(new Error(`Missing E2E command fake: ${command}`));
    }
    const calls = (window.__FISHMUSE_E2E_CALLS__ ??= {});
    (calls[command] ??= []).push(args ?? null);
    return Promise.resolve(structuredClone(commands[command]) as T);
  }
  return invoke<T>(command, args);
}

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
    const turn = await invokeCommand<TurnStarted>("start_ai_turn", { request });
    return { turn, unlisten };
  } catch (error) {
    unlisten();
    throw error;
  }
}

export const getAppStatus = () => invokeCommand<AppStatus>("get_app_status");
export const listenForScanProgress = (listener: (progress: ScanProgress) => void) =>
  listen<ScanProgress>(SCAN_PROGRESS_EVENT, (event) => listener(event.payload));
export const listenForPlaybackState = (listener: (snapshot: PlaybackSnapshot) => void) =>
  listen<PlaybackSnapshot>(PLAYBACK_STATE_EVENT, (event) => listener(event.payload));
export const listenForServiceState = (listener: (state: ServiceStateEvent) => void) =>
  listen<ServiceStateEvent>(SERVICE_STATE_EVENT, (event) => listener(event.payload));
export const chooseLibraryFolders = () =>
  invokeCommand<string[]>("choose_library_folders");
export const startLibraryScan = (roots: string[]) =>
  invokeCommand<{ scan_id: string }>("start_library_scan", { roots });
export const cancelLibraryScan = (scanId: string) =>
  invokeCommand<void>("cancel_library_scan", { scanId });
export const searchLibrary = (query: SearchQuery) =>
  invokeCommand<TrackSummary[]>("search_library", { query });
export const getLibraryItem = (trackId: string) =>
  invokeCommand<LibraryItem | null>("get_library_item", { trackId });
export const cancelAITurn = (turnId: string) =>
  invokeCommand<void>("cancel_ai_turn", { turnId });
export const executePlayback = (command: PlaybackCommand) =>
  invokeCommand<PlaybackSnapshot>("execute_playback", { command });
export const getPlaybackState = () =>
  invokeCommand<PlaybackSnapshot>("get_playback_state");
export const launchPlaybackBackend = () =>
  invokeCommand<void>("launch_playback_backend");
export const configureDeepSeekKey = (apiKey: string) =>
  invokeCommand<void>("configure_deepseek_key", { apiKey: { api_key: apiKey } });
export const deleteDeepSeekKey = () =>
  invokeCommand<void>("delete_deepseek_key");
export const getAISettings = () => invokeCommand<AISettings>("get_ai_settings");
