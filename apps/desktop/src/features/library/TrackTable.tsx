import { useState, type UIEvent } from "react";

import type { TrackSummary } from "../../contracts";

const ROW_HEIGHT = 58;
const VIEWPORT_HEIGHT = 464;
const OVERSCAN = 4;

const duration = (milliseconds: number | null) => {
  if (milliseconds == null) return "—";
  const totalSeconds = Math.floor(milliseconds / 1000);
  return `${Math.floor(totalSeconds / 60)}:${String(totalSeconds % 60).padStart(2, "0")}`;
};

export function TrackTable({ tracks, onPlay }: { tracks: TrackSummary[]; onPlay: (track: TrackSummary) => void }) {
  const [scrollTop, setScrollTop] = useState(0);
  const first = Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - OVERSCAN);
  const count = Math.ceil(VIEWPORT_HEIGHT / ROW_HEIGHT) + OVERSCAN * 2;
  const visible = tracks.slice(first, first + count);
  const onScroll = (event: UIEvent<HTMLDivElement>) => setScrollTop(event.currentTarget.scrollTop);

  return (
    <div className="track-table" role="table" aria-label="Library tracks" aria-rowcount={tracks.length + 1}>
      <div className="track-row track-header" role="row" aria-rowindex={1}>
        <span role="columnheader">Title</span><span role="columnheader">Artist</span>
        <span role="columnheader">Release</span><span role="columnheader">Time</span><span />
      </div>
      <div className="track-viewport" onScroll={onScroll} style={{ height: VIEWPORT_HEIGHT }}>
        <div style={{ height: tracks.length * ROW_HEIGHT, position: "relative" }}>
          <div style={{ transform: `translateY(${first * ROW_HEIGHT}px)` }}>
            {visible.map((track, index) => (
              <div className="track-row" role="row" aria-rowindex={first + index + 2} key={track.id} style={{ height: ROW_HEIGHT }}>
                <strong role="cell">{track.title}</strong>
                <span role="cell">{track.artist_names.join(", ") || "Unknown artist"}</span>
                <span role="cell">{track.release_title ?? "Unknown release"}</span>
                <span role="cell">{duration(track.duration_ms)}</span>
                <button type="button" aria-label={`Play ${track.title}`} disabled={!track.playable} onClick={() => onPlay(track)}>▶</button>
              </div>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}
