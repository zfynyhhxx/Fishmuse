import { usePlaybackState } from "../features/now-playing/usePlaybackState";

const seconds = (milliseconds: number) => Math.max(0, Math.floor(milliseconds / 1000));

export function MiniPlayer() {
  const playback = usePlaybackState();
  const unavailable = playback.status === "unavailable";
  return (
    <aside className="mini-player" aria-label="Mini player">
      <div>
        <strong>{playback.track_id ? "Current track" : "Nothing playing"}</strong>
        <span>{unavailable ? "Playback unavailable" : playback.status}</span>
      </div>
      <span aria-label="Playback position">
        {seconds(playback.position_ms)}s
        {playback.duration_ms == null ? "" : ` / ${seconds(playback.duration_ms)}s`}
      </span>
      {unavailable ? <a href="#/settings">Reconnect in Settings</a> : null}
    </aside>
  );
}
