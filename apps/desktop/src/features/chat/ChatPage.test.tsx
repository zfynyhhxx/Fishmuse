import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { AIEventEnvelope } from "../../contracts";
import { cancelAITurn, getAISettings, startAITurn } from "../../lib/ipc";
import { resetChatSession } from "../../state/chatStore";
import { ChatPage } from "./ChatPage";

vi.mock("../../lib/ipc", () => ({
  cancelAITurn: vi.fn(async () => undefined),
  getAISettings: vi.fn(),
  startAITurn: vi.fn(),
}));

const TURN_IDS = [
  "01999999-9999-7999-8999-999999999901",
  "01999999-9999-7999-8999-999999999902",
  "01999999-9999-7999-8999-999999999903",
  "01999999-9999-7999-8999-999999999904",
] as const;

type WithoutEnvelope<T> = T extends unknown
  ? Omit<T, "contract_version" | "turn_id" | "sequence">
  : never;
type AIEventBody = WithoutEnvelope<AIEventEnvelope>;

function event(
  turnId: string,
  sequence: number,
  body: AIEventBody,
): AIEventEnvelope {
  return { contract_version: 1, turn_id: turnId, sequence, ...body } as AIEventEnvelope;
}

function send(text = "Find something calm") {
  fireEvent.change(screen.getByRole("textbox", { name: "Message FishMuse" }), {
    target: { value: text },
  });
  fireEvent.click(screen.getByRole("button", { name: "Send" }));
}

beforeEach(() => {
  resetChatSession();
  vi.clearAllMocks();
  vi.mocked(getAISettings).mockResolvedValue({
    configured: true,
    provider: "deepseek",
    model: "deepseek-flash",
    service: {
      status: "ready",
      implementation: { id: "deepseek", display_name: "DeepSeek" },
    },
    budget: {
      spent_microunits: 12_345,
      warning_at_microunits: 10_000_000,
      hard_stop_at_microunits: 20_000_000,
    },
  });
});

afterEach(cleanup);

describe("Ask FishMuse", () => {
  it("orders streamed text, shows safe tool activity, and keeps the active conversation across navigation", async () => {
    let publish: ((value: AIEventEnvelope) => void) | undefined;
    vi.mocked(startAITurn).mockImplementation(async (_request, listener) => {
      publish = listener;
      return { turn: { turn_id: TURN_IDS[0] }, unlisten: vi.fn() };
    });

    const first = render(<ChatPage />);
    send();
    await waitFor(() => expect(startAITurn).toHaveBeenCalledTimes(1));

    act(() => {
      publish?.(event(TURN_IDS[0], 1, { event_type: "turn_started" }));
      publish?.(event(TURN_IDS[0], 3, { event_type: "text_delta", payload: { delta: "world" } }));
      publish?.(event(TURN_IDS[0], 3, { event_type: "text_delta", payload: { delta: "tampered duplicate" } }));
      publish?.(event(TURN_IDS[0], 2, { event_type: "text_delta", payload: { delta: "Hello " } }));
      publish?.(event(TURN_IDS[0], 4, {
        event_type: "tool_started",
        payload: { id: "tool-1", name: "search_library" },
      }));
      publish?.(event(TURN_IDS[0], 5, {
        event_type: "tool_finished",
        payload: {
          id: "tool-1",
          name: "search_library",
          result: { path: "C:\\Users\\private\\Music", api_key: "never-render" },
        },
      }));
      publish?.(event(TURN_IDS[0], 6, {
        event_type: "usage",
        payload: { input_tokens: 100, cached_input_tokens: 40, output_tokens: 20 },
      }));
      publish?.(event(TURN_IDS[0], 7, { event_type: "turn_completed" }));
    });

    expect(await screen.findByText("Hello world")).toBeTruthy();
    expect(screen.getByText("Searched your library")).toBeTruthy();
    expect(screen.getByText(/DeepSeek · deepseek-flash/)).toBeTruthy();
    expect(screen.getByText(/Estimated local spend/)).toBeTruthy();
    expect(screen.getByText(/120 tokens/)).toBeTruthy();
    expect(document.body.textContent).not.toContain("C:\\Users");
    expect(document.body.textContent).not.toContain("never-render");

    first.unmount();
    render(<ChatPage />);
    expect(screen.getByText("Hello world")).toBeTruthy();
    expect(screen.getByText("Find something calm")).toBeTruthy();
  });

  it("cancels only the current turn, retries, ignores an old turn, and explains tool/auth/rate failures", async () => {
    const listeners: Array<(value: AIEventEnvelope) => void> = [];
    let starts = 0;
    vi.mocked(startAITurn).mockImplementation(async (_request, listener) => {
      const turnId = TURN_IDS[starts++];
      listeners.push(listener);
      return { turn: { turn_id: turnId }, unlisten: vi.fn() };
    });

    render(<ChatPage />);
    send("Play the next track");
    await screen.findByRole("button", { name: "Stop generating" });
    fireEvent.click(screen.getByRole("button", { name: "Stop generating" }));
    expect(cancelAITurn).toHaveBeenCalledWith(TURN_IDS[0]);
    act(() => listeners[0](event(TURN_IDS[0], 1, {
      event_type: "turn_failed",
      payload: { reason: "cancelled" },
    })));
    expect(await screen.findByText("Response cancelled.")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    await waitFor(() => expect(startAITurn).toHaveBeenCalledTimes(2));
    act(() => {
      listeners[1](event(TURN_IDS[1], 1, { event_type: "turn_started" }));
      listeners[0](event(TURN_IDS[0], 2, {
        event_type: "text_delta",
        payload: { delta: "late old response" },
      }));
      listeners[1](event(TURN_IDS[1], 2, {
        event_type: "turn_failed",
        payload: { reason: "tool_limit" },
      }));
    });
    expect(await screen.findByText(/six tool actions/)).toBeTruthy();
    expect(document.body.textContent).not.toContain("late old response");

    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    await waitFor(() => expect(startAITurn).toHaveBeenCalledTimes(3));
    act(() => listeners[2](event(TURN_IDS[2], 1, {
      event_type: "turn_failed",
      payload: { reason: "provider_unauthorized" },
    })));
    expect(await screen.findByRole("link", { name: "Reconfigure API key" })).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    await waitFor(() => expect(startAITurn).toHaveBeenCalledTimes(4));
    act(() => listeners[3](event(TURN_IDS[3], 1, {
      event_type: "turn_failed",
      payload: { reason: "provider_rate_limited" },
    })));
    expect(await screen.findByText(/rate limit.*try again later/i)).toBeTruthy();
  });
});
