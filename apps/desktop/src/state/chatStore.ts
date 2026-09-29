import type { AIEventEnvelope } from "../contracts";
import { newUuidV7 } from "../lib/uuid";

export type ChatStatus = "idle" | "starting" | "streaming" | "completed" | "failed" | "cancelled";

export type ChatMessage = {
  id: string;
  role: "user" | "assistant";
  text: string;
};

export type ChatToolActivity = {
  id: string;
  name: string;
  status: "running" | "completed";
};

export type ChatUsage = {
  inputTokens: number;
  cachedInputTokens: number;
  outputTokens: number;
};

export type ChatFailure =
  | "provider"
  | "provider_unauthorized"
  | "provider_rate_limited"
  | "tool"
  | "tool_limit"
  | "cancelled"
  | "start";

export type ChatState = {
  conversationId: string;
  messages: ChatMessage[];
  tools: ChatToolActivity[];
  status: ChatStatus;
  activeTurnId: string | null;
  lastUserText: string | null;
  usage: ChatUsage | null;
  failure: ChatFailure | null;
};

const initialState = (): ChatState => ({
  conversationId: newUuidV7(),
  messages: [],
  tools: [],
  status: "idle",
  activeTurnId: null,
  lastUserText: null,
  usage: null,
  failure: null,
});

export function createChatStore() {
  let state = initialState();
  let lastSequence = 0;
  let assistantMessageId: string | null = null;
  const buffered = new Map<number, AIEventEnvelope>();
  const listeners = new Set<() => void>();
  const notify = () => listeners.forEach((listener) => listener());
  const publish = (next: ChatState) => {
    state = next;
    notify();
  };

  const reduceEvent = (event: AIEventEnvelope) => {
    switch (event.event_type) {
      case "turn_started":
        publish({ ...state, status: "streaming" });
        break;
      case "text_delta":
        publish({
          ...state,
          status: "streaming",
          messages: state.messages.map((message) => message.id === assistantMessageId
            ? { ...message, text: message.text + event.payload.delta }
            : message),
        });
        break;
      case "tool_started":
        publish({
          ...state,
          status: "streaming",
          tools: [...state.tools, { id: event.payload.id, name: event.payload.name, status: "running" }],
        });
        break;
      case "tool_finished":
        publish({
          ...state,
          tools: state.tools.map((tool) => tool.id === event.payload.id
            ? { ...tool, status: "completed" }
            : tool),
        });
        break;
      case "usage":
        publish({
          ...state,
          usage: {
            inputTokens: (state.usage?.inputTokens ?? 0) + event.payload.input_tokens,
            cachedInputTokens: (state.usage?.cachedInputTokens ?? 0) + event.payload.cached_input_tokens,
            outputTokens: (state.usage?.outputTokens ?? 0) + event.payload.output_tokens,
          },
        });
        break;
      case "turn_completed":
        publish({ ...state, status: "completed", activeTurnId: null });
        break;
      case "turn_failed": {
        const cancelled = event.payload.reason === "cancelled";
        publish({
          ...state,
          status: cancelled ? "cancelled" : "failed",
          activeTurnId: null,
          failure: event.payload.reason,
        });
        break;
      }
    }
  };

  return {
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    getSnapshot() {
      return state;
    },
    begin(userText: string, retry = false) {
      lastSequence = 0;
      buffered.clear();
      assistantMessageId = newUuidV7();
      const messages = retry
        ? [...state.messages, { id: assistantMessageId, role: "assistant" as const, text: "" }]
        : [
            ...state.messages,
            { id: newUuidV7(), role: "user" as const, text: userText },
            { id: assistantMessageId, role: "assistant" as const, text: "" },
          ];
      publish({
        ...state,
        messages,
        tools: [],
        status: "starting",
        activeTurnId: null,
        lastUserText: userText,
        usage: null,
        failure: null,
      });
    },
    activate(conversationId: string, turnId: string) {
      if (conversationId !== state.conversationId) return false;
      publish({ ...state, activeTurnId: turnId, status: "streaming" });
      return true;
    },
    apply(conversationId: string, event: AIEventEnvelope) {
      if (
        conversationId !== state.conversationId
        ||
        event.turn_id !== state.activeTurnId
        || event.sequence <= lastSequence
        || buffered.has(event.sequence)
      ) return;
      buffered.set(event.sequence, event);
      let next = buffered.get(lastSequence + 1);
      while (next) {
        buffered.delete(lastSequence + 1);
        lastSequence += 1;
        reduceEvent(next);
        if (state.status === "completed" || state.status === "failed" || state.status === "cancelled") {
          buffered.clear();
          break;
        }
        next = buffered.get(lastSequence + 1);
      }
    },
    failStart(conversationId: string) {
      if (conversationId !== state.conversationId) return;
      publish({ ...state, status: "failed", activeTurnId: null, failure: "start" });
    },
    reset() {
      state = initialState();
      lastSequence = 0;
      assistantMessageId = null;
      buffered.clear();
      notify();
    },
  };
}

export const chatStore = createChatStore();

export function resetChatSession() {
  chatStore.reset();
}
