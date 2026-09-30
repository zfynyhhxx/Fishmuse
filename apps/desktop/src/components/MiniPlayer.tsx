import { useRef, useState } from "react";

import type { PlaybackAction } from "../contracts";
import { runPlaybackAction } from "../lib/ipc";
import { playbackStore } from "../state/playbackStore";
import { usePlaybackState } from "../features/now-playing/usePlaybackState";

const seconds = (milliseconds: number) => Math.max(0, Math.floor(milliseconds / 1000));

export function MiniPlayer() {
  const playback = usePlaybackState();
  const [pending, setPending] = useState<Set<PlaybackAction["kind"]>>(() => new Set());
  const [error, setError] = useState(false);
  const inFlight = useRef(new Set<PlaybackAction["kind"]>());
  const unavailable = playback.status === "unavailable";
  const toggle: PlaybackAction = playback.status === "playing" ? { kind: "pause" } : { kind: "resume" };
  const title = playback.track?.title ?? (playback.external ? "External playback" : "Nothing playing");
  const subtitle = playback.track
    ? playback.track.artist_names.join(", ") || "Unknown artist"
    : unavailable ? "Playback unavailable" : playback.status;

  const run = async (action: PlaybackAction) => {
    if (inFlight.current.has(action.kind)) return;
    inFlight.current.add(action.kind);
    setPending(new Set(inFlight.current));
    setError(false);
    try {
      playbackStore.acceptActionResult(await runPlaybackAction(action));
    } catch {
      setError(true);
    } finally {
      inFlight.current.delete(action.kind);
      setPending(new Set(inFlight.current));
    }
  };

  return (
    <aside className="mini-player" aria-label="Mini player">
      <div className="mini-track">
        <strong>{title}</strong>
        <span>{subtitle}</span>
      </div>
      <span aria-label="Playback position">
        {seconds(playback.position_ms)}s
        {playback.duration_ms == null ? "" : ` / ${seconds(playback.duration_ms)}s`}
      </span>
      <div className="mini-controls">
        <button
          type="button"
          className="secondary"
          aria-label="Previous track"
          disabled={unavailable || !playback.queue.can_previous || pending.has("previous")}
          onClick={() => void run({ kind: "previous" })}
        >◀</button>
        <button
          type="button"
          aria-label={toggle.kind === "pause" ? "Pause" : "Resume"}
          disabled={unavailable || pending.has(toggle.kind) || (toggle.kind === "resume" && !playback.track && !playback.external)}
          onClick={() => void run(toggle)}
        >{toggle.kind === "pause" ? "Ⅱ" : "▶"}</button>
        <button
          type="button"
          className="secondary"
          aria-label="Next track"
          disabled={unavailable || !playback.queue.can_next || pending.has("next")}
          onClick={() => void run({ kind: "next" })}
        >▶</button>
      </div>
      {error ? <a href="#/settings">Playback unavailable · Advanced diagnostics</a> : null}
    </aside>
  );
}
