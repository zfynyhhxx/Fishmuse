import { useRef, useState } from "react";

import type { PlaybackCommand, PlaybackSnapshot } from "../../contracts";
import { executePlayback } from "../../lib/ipc";
import { newUuidV7 } from "../../lib/uuid";
import { playbackStore } from "../../state/playbackStore";

type ControlKind = "pause" | "resume" | "seek" | "skip_next";

export function PlaybackControls({ snapshot }: { snapshot: PlaybackSnapshot }) {
  const [seekPosition, setSeekPosition] = useState<number | null>(null);
  const [pending, setPending] = useState<Set<ControlKind>>(() => new Set());
  const [error, setError] = useState<string | null>(null);
  const inFlight = useRef(new Set<ControlKind>());

  const run = async (kind: ControlKind, positionMs?: number) => {
    if (inFlight.current.has(kind)) return;
    inFlight.current.add(kind);
    setPending(new Set(inFlight.current));
    setError(null);
    const operation_id = newUuidV7();
    const command: PlaybackCommand = kind === "seek"
      ? { kind, position_ms: positionMs ?? snapshot.position_ms, operation_id }
      : { kind, operation_id };
    try {
      playbackStore.accept(await executePlayback(command));
      if (kind === "seek") setSeekPosition(null);
    } catch {
      setError("Playback control is unavailable. Check the playback service in Settings.");
    } finally {
      inFlight.current.delete(kind);
      setPending(new Set(inFlight.current));
    }
  };

  const unavailable = snapshot.status === "unavailable";
  return (
    <div className="playback-controls">
      <div className="button-row">
        {snapshot.status === "playing" ? (
          <button type="button" disabled={pending.has("pause") || unavailable} onClick={() => void run("pause")}>Pause</button>
        ) : (
          <button type="button" disabled={pending.has("resume") || unavailable || !snapshot.track_id} onClick={() => void run("resume")}>Resume</button>
        )}
        <button type="button" className="secondary" disabled={pending.has("skip_next") || unavailable} onClick={() => void run("skip_next")}>Next track</button>
      </div>
      <label className="seek-control">
        <span>Position</span>
        <input
          aria-label="Seek position"
          type="range"
          min={0}
          max={snapshot.duration_ms ?? Math.max(snapshot.position_ms, 1)}
          value={Math.min(seekPosition ?? snapshot.position_ms, snapshot.duration_ms ?? seekPosition ?? snapshot.position_ms)}
          disabled={unavailable || snapshot.duration_ms == null}
          onChange={(event) => setSeekPosition(Number(event.target.value))}
        />
        <button type="button" className="secondary" disabled={pending.has("seek") || unavailable || snapshot.duration_ms == null} onClick={() => void run("seek", seekPosition ?? snapshot.position_ms)}>Seek</button>
      </label>
      {error ? <p className="error-banner" role="alert">{error}</p> : null}
    </div>
  );
}
