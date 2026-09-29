import { useEffect, useRef, useState, useSyncExternalStore } from "react";

import type { AIEventEnvelope, AISettings } from "../../contracts";
import { cancelAITurn, getAISettings, startAITurn } from "../../lib/ipc";
import { chatStore } from "../../state/chatStore";

let activeListener: { turnId: string; unlisten: () => void } | null = null;

async function start(userText: string, retry: boolean) {
  activeListener?.unlisten();
  activeListener = null;
  chatStore.begin(userText, retry);
  const conversationId = chatStore.getSnapshot().conversationId;
  const waiting: AIEventEnvelope[] = [];
  let turnId: string | null = null;
  let unlisten: (() => void) | null = null;
  const receive = (event: AIEventEnvelope) => {
    if (!turnId) {
      waiting.push(event);
      return;
    }
    chatStore.apply(conversationId, event);
    const status = chatStore.getSnapshot().status;
    if (status === "completed" || status === "failed" || status === "cancelled") {
      unlisten?.();
      activeListener = null;
    }
  };
  try {
    const result = await startAITurn({
      conversation_id: conversationId,
      user_text: userText,
      context: null,
    }, receive);
    turnId = result.turn.turn_id;
    unlisten = result.unlisten;
    if (!chatStore.activate(conversationId, turnId)) {
      result.unlisten();
      return;
    }
    waiting.sort((left, right) => left.sequence - right.sequence).forEach(receive);
    const status = chatStore.getSnapshot().status;
    if (status === "completed" || status === "failed" || status === "cancelled") {
      result.unlisten();
    } else {
      activeListener = { turnId, unlisten: result.unlisten };
    }
  } catch {
    chatStore.failStart(conversationId);
  }
}

export function useAgentTurn() {
  const state = useSyncExternalStore(chatStore.subscribe, chatStore.getSnapshot);
  const [settings, setSettings] = useState<AISettings | null>(null);
  const settingsRequest = useRef(0);

  useEffect(() => {
    if (state.status !== "idle" && state.status !== "completed" && state.status !== "failed") return;
    const request = ++settingsRequest.current;
    void getAISettings()
      .then((next) => {
        if (request === settingsRequest.current) setSettings(next);
      })
      .catch(() => {
        if (request === settingsRequest.current) setSettings(null);
      });
  }, [state.status]);

  return {
    state,
    settings,
    send: (text: string) => start(text, false),
    retry: () => state.lastUserText ? start(state.lastUserText, true) : Promise.resolve(),
    cancel: async () => {
      const turnId = chatStore.getSnapshot().activeTurnId;
      if (turnId) {
        try {
          await cancelAITurn(turnId);
        } catch {
          // A terminal event may win the race with the cancellation command.
        }
      }
    },
  };
}
