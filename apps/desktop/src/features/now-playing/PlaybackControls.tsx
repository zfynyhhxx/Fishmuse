import { useRef, useState } from "react";

import type { PlaybackAction, PlaybackView } from "../../contracts";
import { retryPlaybackService, runPlaybackAction } from "../../lib/ipc";
import { playbackStore } from "../../state/playbackStore";
import { PlaybackQueue } from "./PlaybackQueue";

export function PlaybackControls({ snapshot }: { snapshot: PlaybackView }) {
  const [seekPosition, setSeekPosition] = useState<number | null>(null);
  const [volumeDraft, setVolumeDraft] = useState<number | null>(null);
  const [pending, setPending] = useState<Set<PlaybackAction["kind"]>>(() => new Set());
  const [error, setError] = useState<string | null>(null);
  const inFlight = useRef(new Set<PlaybackAction["kind"]>());

  const run = async (action: PlaybackAction) => {
    if (inFlight.current.has(action.kind)) return;
    inFlight.current.add(action.kind);
    setPending(new Set(inFlight.current));
    setError(null);
    try {
      playbackStore.acceptActionResult(await runPlaybackAction(action));
      if (action.kind === "seek") setSeekPosition(null);
      if (action.kind === "setVolume") setVolumeDraft(null);
    } catch {
      setError("Playback control is unavailable. Please retry or open advanced diagnostics.");
    } finally {
      inFlight.current.delete(action.kind);
      setPending(new Set(inFlight.current));
    }
  };

  const retry = async () => {
    try {
      await retryPlaybackService();
      setError(null);
    } catch {
      setError("Playback control is unavailable. Please retry or open advanced diagnostics.");
    }
  };

  const unavailable = snapshot.status === "unavailable";
  const hasActiveSource = snapshot.track != null || snapshot.external;
  const toggleAction: PlaybackAction = snapshot.status === "playing"
    ? { kind: "pause" }
    : { kind: "resume" };
  const toggleLabel = snapshot.status === "playing" ? "Pause" : "Resume";
  const muteVolume = snapshot.muted ? playbackStore.lastNonZeroVolume() : 0;
  const volumePercent = volumeDraft ?? Math.round(snapshot.volume * 100);

  return (
    <div className="playback-controls">
      <div className="button-row transport-controls">
        <button
          type="button"
          className="secondary"
          disabled={pending.has("previous") || unavailable || !snapshot.queue.can_previous}
          onClick={() => void run({ kind: "previous" })}
        >Previous track</button>
        <button
          type="button"
          disabled={pending.has(toggleAction.kind) || unavailable || (!hasActiveSource && toggleAction.kind === "resume")}
          onClick={() => void run(toggleAction)}
        >{toggleLabel}</button>
        <button
          type="button"
          className="secondary"
          disabled={pending.has("next") || unavailable || !snapshot.queue.can_next}
          onClick={() => void run({ kind: "next" })}
        >Next track</button>
        <button
          type="button"
          className="secondary"
          disabled={pending.has("stop") || unavailable || snapshot.status === "stopped"}
          onClick={() => void run({ kind: "stop" })}
        >Stop</button>
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
        <button
          type="button"
          className="secondary"
          disabled={pending.has("seek") || unavailable || snapshot.duration_ms == null}
          onClick={() => void run({ kind: "seek", positionMs: seekPosition ?? snapshot.position_ms })}
        >Seek</button>
      </label>
      <label className="volume-control">
        <span>Volume</span>
        <input
          aria-label="Volume"
          type="range"
          min={0}
          max={100}
          value={volumePercent}
          disabled={unavailable}
          onChange={(event) => setVolumeDraft(Number(event.target.value))}
        />
        <button
          type="button"
          className="secondary"
          disabled={pending.has("setVolume") || unavailable}
          onClick={() => void run({ kind: "setVolume", volume: volumePercent / 100 })}
        >Set volume</button>
        <button
          type="button"
          className="secondary"
          disabled={pending.has("setVolume") || unavailable}
          onClick={() => void run({ kind: "setVolume", volume: muteVolume })}
        >{snapshot.muted ? "Unmute" : "Mute"}</button>
      </label>
      <PlaybackQueue snapshot={snapshot} run={run} pending={pending} />
      {error ? (
        <div className="error-banner" role="alert">
          <p>{error}</p>
          <div className="button-row">
            <button type="button" onClick={() => void retry()}>Retry playback</button>
            <a className="button secondary" href="#/settings">Advanced diagnostics</a>
          </div>
        </div>
      ) : null}
    </div>
  );
}
