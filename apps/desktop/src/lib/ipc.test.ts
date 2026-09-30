import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  AI_EVENT,
  parseAIEventEnvelope,
  retryPlaybackService,
  runPlaybackAction,
  startAITurn,
} from "./ipc";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

const mockedInvoke = vi.mocked(invoke);
const mockedListen = vi.mocked(listen);

beforeEach(() => {
  mockedInvoke.mockReset();
  mockedListen.mockReset();
});

describe("desktop IPC boundary", () => {
  it("routes every playback action through one typed entry with fresh UUIDv7 operations", async () => {
    mockedInvoke.mockImplementation(async (command) => {
      if (command === "execute_queue_command") {
        return { track_ids: [], current_index: null, can_previous: false, can_next: false };
      }
      return {
        revision: 1,
        status: "paused",
        position_ms: 0,
        duration_ms: null,
        volume: 0.5,
        muted: false,
        track: null,
        queue: { track_ids: [], current_index: null, can_previous: false, can_next: false },
        external: false,
      };
    });

    await runPlaybackAction({ kind: "playNow", trackId: "track-a", context: ["track-a", "track-b"] });
    await runPlaybackAction({ kind: "pause" });
    await runPlaybackAction({ kind: "resume" });
    await runPlaybackAction({ kind: "stop" });
    await runPlaybackAction({ kind: "seek", positionMs: 12_000 });
    await runPlaybackAction({ kind: "setVolume", volume: 0.5 });
    await runPlaybackAction({ kind: "previous" });
    await runPlaybackAction({ kind: "next" });

    const commands = mockedInvoke.mock.calls.map(([, args]) => (
      args as { command: Record<string, unknown> }
    ).command);
    const operationIds = commands
      .map((command) => command.operation_id)
      .filter((value): value is string => typeof value === "string");
    expect(operationIds).toHaveLength(6);
    expect(new Set(operationIds).size).toBe(6);
    for (const operationId of operationIds) {
      expect(operationId).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i);
    }
    expect(mockedInvoke.mock.calls.map(([command]) => command)).toEqual([
      "execute_queue_command",
      "execute_playback",
      "execute_playback",
      "execute_playback",
      "execute_playback",
      "execute_playback",
      "execute_queue_command",
      "execute_queue_command",
    ]);
  });

  it("invokes only the advanced playback service retry", async () => {
    mockedInvoke.mockResolvedValue(undefined);

    await retryPlaybackService();

    expect(mockedInvoke).toHaveBeenCalledWith("retry_playback_service", undefined);
  });

  it("subscribes before starting an AI turn and forwards optional structured context", async () => {
    const order: string[] = [];
    const unlisten = vi.fn();
    mockedListen.mockImplementation(async () => {
      order.push("listen");
      return unlisten;
    });
    mockedInvoke.mockImplementation(async () => {
      order.push("invoke");
      return { turn_id: "01999999-9999-7999-8999-999999999999" };
    });

    const context = {
      contract_version: 1 as const,
      current_view: "library" as const,
      selected_entity: null,
      selected_text: "selected words",
      now_playing: null,
    };
    const result = await startAITurn(
      {
        conversation_id: "01999999-9999-7999-8999-999999999998",
        user_text: "explain",
        context,
      },
      vi.fn(),
    );

    expect(order).toEqual(["listen", "invoke"]);
    expect(mockedListen).toHaveBeenCalledWith(AI_EVENT, expect.any(Function));
    expect(mockedInvoke).toHaveBeenCalledWith("start_ai_turn", {
      request: expect.objectContaining({ context }),
    });
    expect(result.turn.turn_id).toMatch(/^[0-9a-f-]+$/);
    result.unlisten();
    expect(unlisten).toHaveBeenCalledOnce();
  });

  it("accepts only the versioned sequenced application AI envelope", () => {
    const event = parseAIEventEnvelope({
      contract_version: 1,
      turn_id: "01999999-9999-7999-8999-999999999999",
      sequence: 2,
      event_type: "text_delta",
      payload: { delta: "hi" },
    });
    expect(event.event_type).toBe("text_delta");
    expect(() => parseAIEventEnvelope({ ...event, contract_version: 2 })).toThrow();
    expect(() => parseAIEventEnvelope({ ...event, sequence: 0 })).toThrow();
    expect(JSON.stringify(event)).not.toContain("DeepSeek");
  });
});
