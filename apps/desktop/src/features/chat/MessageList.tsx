import type { ChatMessage } from "../../state/chatStore";

export function MessageList({ messages }: { messages: ChatMessage[] }) {
  if (messages.length === 0) {
    return <div className="empty-state"><h2>Ask about your music</h2><p>Search your local library, compare releases, or control playback.</p></div>;
  }
  return (
    <ol className="message-list" aria-live="polite">
      {messages.filter((message) => message.role === "user" || message.text).map((message) => (
        <li className={`message message-${message.role}`} key={message.id}>
          <span>{message.role === "user" ? "You" : "FishMuse"}</span>
          <p>{message.text}</p>
        </li>
      ))}
    </ol>
  );
}
