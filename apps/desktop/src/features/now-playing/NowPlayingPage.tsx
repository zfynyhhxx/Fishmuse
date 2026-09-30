import { useEffect, useState } from "react";

import { getTrackArtwork } from "../../lib/ipc";
import { PlaybackControls } from "./PlaybackControls";
import { usePlaybackState } from "./usePlaybackState";

const formatTime = (milliseconds: number) => {
  const seconds = Math.max(0, Math.floor(milliseconds / 1_000));
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
};

export function NowPlayingPage() {
  const snapshot = usePlaybackState();
  const [resolvedArtwork, setResolvedArtwork] = useState<{
    trackId: string;
    dataUrl: string | null;
  } | null>(null);
  const unavailable = snapshot.status === "unavailable";
  const track = snapshot.track;

  useEffect(() => {
    let current = true;
    if (!track?.artwork_available) return () => { current = false; };
    void getTrackArtwork(track.id)
      .then((result) => {
        if (current) setResolvedArtwork({ trackId: track.id, dataUrl: result?.data_url ?? null });
      })
      .catch(() => {
        if (current) setResolvedArtwork({ trackId: track.id, dataUrl: null });
      });
    return () => { current = false; };
  }, [track?.artwork_available, track?.id]);

  const title = track?.title ?? (snapshot.external ? "External playback" : "Nothing playing");
  const artwork = track && resolvedArtwork?.trackId === track.id
    ? resolvedArtwork.dataUrl
    : null;
  return (
    <section className="page-stack" aria-labelledby="now-playing-title">
      <div className="page-heading">
        <div><p className="eyebrow">Playback</p><h1 id="now-playing-title">Now Playing</h1></div>
        <span className={`playback-state playback-state-${snapshot.status}`}>{snapshot.status}</span>
      </div>
      <section className="now-playing-card">
        {artwork ? (
          <img className="cover-artwork" src={artwork} alt={`Artwork for ${title}`} />
        ) : (
          <div className="cover-placeholder" role="img" aria-label="No artwork available">♫</div>
        )}
        <div className="now-playing-details">
          <h2>{title}</h2>
          {track ? (
            <div className="track-metadata">
              <p>{track.artist_names.join(", ") || "Unknown artist"}</p>
              <p>{track.release_title ?? "Unknown release"}</p>
            </div>
          ) : snapshot.external ? (
            <p>This track is playing outside the FishMuse library.</p>
          ) : (
            <p>Choose a playable track from your Library.</p>
          )}
          <p>{formatTime(snapshot.position_ms)} / {snapshot.duration_ms == null ? "--:--" : formatTime(snapshot.duration_ms)}</p>
          {unavailable ? <p className="warning-banner">Playback is disconnected. The last known track is retained while controls are unavailable.</p> : null}
          <PlaybackControls snapshot={snapshot} />
        </div>
      </section>
    </section>
  );
}
