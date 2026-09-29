import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { AI_EVENT, launchPlaybackBackend, parseAIEventEnvelope, startAITurn } from "./ipc";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

const mockedInvoke = vi.mocked(invoke);
const mockedListen = vi.mocked(listen);

beforeEach(() => {
  mockedInvoke.mockReset();
  mockedListen.mockReset();
});

describe("desktop IPC boundary", () => {
  it("invokes the playback backend launcher", async () => {
    mockedInvoke.mockResolvedValue(undefined);

    await launchPlaybackBackend();

    expect(mockedInvoke).toHaveBeenCalledWith("launch_playback_backend", undefined);
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
