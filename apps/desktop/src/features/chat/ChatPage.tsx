import { useState, type FormEvent } from "react";

import type { ChatFailure } from "../../state/chatStore";
import { MessageList } from "./MessageList";
import { ToolActivity } from "./ToolActivity";
import { useAgentTurn } from "./useAgentTurn";

function failureMessage(failure: ChatFailure | null) {
  switch (failure) {
    case "cancelled": return "Response cancelled.";
    case "tool_limit": return "FishMuse stopped safely after six tool actions. Narrow the request and retry.";
    case "provider_unauthorized": return "The AI credential was rejected.";
    case "provider_rate_limited": return "The AI provider rate limit was reached; try again later.";
    case "tool": return "A music action could not be completed. Nothing was rolled back.";
    case "provider": return "The AI service was interrupted. You can retry this response.";
    case "start": return "FishMuse could not start the response. Check Settings and retry.";
    default: return null;
  }
}

const yuan = (microYuan: number) => (microYuan / 1_000_000).toFixed(2);

export function ChatPage() {
  const { state, settings, send, retry, cancel } = useAgentTurn();
  const [draft, setDraft] = useState("");
  const busy = state.status === "starting" || state.status === "streaming";
  const error = failureMessage(state.failure);
  const submit = (event: FormEvent) => {
    event.preventDefault();
    const text = draft.trim();
    if (!text || busy) return;
    setDraft("");
    void send(text);
  };
  const totalTokens = state.usage
    ? state.usage.inputTokens + state.usage.outputTokens
    : null;

  return (
    <section className="chat-page" aria-labelledby="chat-title">
      <header className="page-heading">
        <div><p className="eyebrow">Your local music guide</p><h1 id="chat-title">Ask FishMuse</h1></div>
        {settings ? <span className="model-label">{settings.service.implementation?.display_name ?? settings.provider} · {settings.model}</span> : null}
      </header>
      <div className="chat-scroll">
        <MessageList messages={state.messages} />
        <ToolActivity activities={state.tools} />
        {busy ? <p role="status">FishMuse is thinking…</p> : null}
        {error ? (
          <div className={state.failure === "cancelled" ? "warning-banner" : "error-banner"} role="alert">
            <p>{error}</p>
            {state.failure === "provider_unauthorized" ? <a href="#/settings">Reconfigure API key</a> : null}
            <button type="button" className="secondary" onClick={() => void retry()}>Retry</button>
          </div>
        ) : null}
        {settings || totalTokens != null ? (
          <details className="usage-details">
            <summary>Turn usage and cost</summary>
            {totalTokens != null ? <p>{totalTokens} tokens ({state.usage?.cachedInputTokens ?? 0} cached input)</p> : null}
            {settings ? <p>Estimated local spend ¥{yuan(settings.budget.spent_microunits)}. Provider billing may differ.</p> : null}
          </details>
        ) : null}
      </div>
      <form className="chat-composer" onSubmit={submit}>
        <label>
          <span>Message FishMuse</span>
          <textarea aria-label="Message FishMuse" value={draft} disabled={busy} onChange={(event) => setDraft(event.target.value)} rows={3} />
        </label>
        {busy ? (
          <button type="button" className="secondary" onClick={() => void cancel()}>Stop generating</button>
        ) : (
          <button type="submit" disabled={!draft.trim()}>Send</button>
        )}
      </form>
    </section>
  );
}
