export const AI_APPLICATION_CONTRACT_VERSION = 1 as const;

export type ServiceImplementation = {
  id: string;
  display_name: string;
};

export type PlaybackServiceState = {
  status: "ready" | "disconnected" | "unavailable";
  implementation: ServiceImplementation | null;
};

export type AIServiceState = {
  status: "ready" | "not_configured" | "unavailable";
  implementation: ServiceImplementation | null;
};

export type AppStatus = {
  version: string;
  database: "ready" | "unavailable";
  playback: PlaybackServiceState;
  ai: AIServiceState;
};

export type SelectedEntityContext =
  | { entity_type: "artist"; artist_id: string; display_name: string }
  | { entity_type: "release"; release_id: string; display_title: string }
  | { entity_type: "track"; track_id: string; display_title: string };

export type ContextEnvelope = {
  contract_version: typeof AI_APPLICATION_CONTRACT_VERSION;
  current_view: "library" | "ask_fish_muse" | "now_playing" | "settings" | null;
  selected_entity: SelectedEntityContext | null;
  selected_text: string | null;
  now_playing: {
    track_id: string | null;
    status: "stopped" | "loading" | "playing" | "paused" | "unavailable";
    position_ms: number;
    duration_ms: number | null;
  } | null;
};

export type StartTurnRequest = {
  conversation_id: string;
  user_text: string;
  context: ContextEnvelope | null;
};

export type TurnStarted = { turn_id: string };

type AIEventBody =
  | { event_type: "turn_started" }
  | { event_type: "text_delta"; payload: { delta: string } }
  | { event_type: "tool_started"; payload: { id: string; name: string } }
  | {
      event_type: "tool_finished";
      payload: { id: string; name: string; result: unknown };
    }
  | {
      event_type: "usage";
      payload: {
        input_tokens: number;
        cached_input_tokens: number;
        output_tokens: number;
      };
    }
  | { event_type: "turn_completed" }
  | {
      event_type: "turn_failed";
      payload: { reason: "provider" | "tool" | "tool_limit" | "cancelled" };
    };

export type AIEventEnvelope = {
  contract_version: typeof AI_APPLICATION_CONTRACT_VERSION;
  turn_id: string;
  sequence: number;
} & AIEventBody;

export type SearchQuery = {
  text: string;
  artist: string | null;
  release: string | null;
  limit: number;
};

export type PlaybackCommand =
  | { kind: "play"; track_id: string; operation_id: string }
  | { kind: "pause"; operation_id: string }
  | { kind: "resume"; operation_id: string }
  | { kind: "seek"; position_ms: number; operation_id: string }
  | { kind: "skip_next"; operation_id: string };

export type PlaybackSnapshot = {
  revision: number;
  status: "stopped" | "loading" | "playing" | "paused" | "unavailable";
  track_id: string | null;
  position_ms: number;
  duration_ms: number | null;
};

export type CommandError = {
  code: string;
  category: string;
  user_message: string;
  retryable: boolean;
  suggested_action: string | null;
};

export type ScanProgress = {
  scan_id: string;
  discovered: number;
  parsed: number;
  unchanged: number;
  failed: number;
  status: "running" | "completed" | "cancelled" | "failed";
  error: CommandError | null;
};
