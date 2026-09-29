import { PlaybackControls } from "./PlaybackControls";
import { usePlaybackState } from "./usePlaybackState";

const formatTime = (milliseconds: number) => {
  const seconds = Math.max(0, Math.floor(milliseconds / 1_000));
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
};

export function NowPlayingPage() {
  const snapshot = usePlaybackState();
  const unavailable = snapshot.status === "unavailable";
  return (
    <section className="page-stack" aria-labelledby="now-playing-title">
      <div className="page-heading">
        <div><p className="eyebrow">Playback</p><h1 id="now-playing-title">Now Playing</h1></div>
        <span className={`playback-state playback-state-${snapshot.status}`}>{snapshot.status}</span>
      </div>
      <section className="now-playing-card">
        <div className="cover-placeholder" aria-hidden="true">♫</div>
        <div className="now-playing-details">
          <h2>{snapshot.track_id ? "Current track" : "Nothing playing"}</h2>
          {snapshot.track_id ? <code>{snapshot.track_id}</code> : <p>Choose a playable track from your Library.</p>}
          <p>{formatTime(snapshot.position_ms)} / {snapshot.duration_ms == null ? "--:--" : formatTime(snapshot.duration_ms)}</p>
          {unavailable ? <p className="warning-banner">Playback is disconnected. The last known track is retained while controls are unavailable.</p> : null}
          <PlaybackControls snapshot={snapshot} />
        </div>
      </section>
    </section>
  );
}
