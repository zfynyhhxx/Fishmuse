import { useEffect, useLayoutEffect, useRef, useState, type UIEvent } from "react";

import type { TrackSummary } from "../../contracts";

const ROW_HEIGHT = 58;
const OVERSCAN = 4;

const duration = (milliseconds: number | null) => {
  if (milliseconds == null) return "—";
  const totalSeconds = Math.floor(milliseconds / 1000);
  return `${Math.floor(totalSeconds / 60)}:${String(totalSeconds % 60).padStart(2, "0")}`;
};

type TrackTableProps = {
  tracks: TrackSummary[];
  onPlay: (track: TrackSummary) => void;
  onAdd?: (track: TrackSummary) => void;
  hasMore: boolean;
  loadingMore: boolean;
  onLoadMore: () => void;
};

export function TrackTable({ tracks, onPlay, onAdd, hasMore, loadingMore, onLoadMore }: TrackTableProps) {
  const [scrollTop, setScrollTop] = useState(0);
  const [viewportHeight, setViewportHeight] = useState(ROW_HEIGHT);
  const viewport = useRef<HTMLDivElement>(null);
  const sentinel = useRef<HTMLDivElement>(null);
  const first = Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - OVERSCAN);
  const count = Math.ceil(viewportHeight / ROW_HEIGHT) + OVERSCAN * 2;
  const visible = tracks.slice(first, first + count);
  const onScroll = (event: UIEvent<HTMLDivElement>) => setScrollTop(event.currentTarget.scrollTop);

  useLayoutEffect(() => {
    const element = viewport.current;
    if (!element) return;
    const observer = new ResizeObserver(([entry]) => {
      setViewportHeight(Math.max(ROW_HEIGHT, Math.floor(entry.contentRect.height)));
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    const root = viewport.current;
    const target = sentinel.current;
    if (!root || !target || !hasMore || loadingMore) return;
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((entry) => entry.isIntersecting)) onLoadMore();
    }, { root, rootMargin: `0px 0px ${ROW_HEIGHT * OVERSCAN}px` });
    observer.observe(target);
    return () => observer.disconnect();
  }, [hasMore, loadingMore, onLoadMore]);

  return (
    <div className="track-table" role="table" aria-label="Library tracks" aria-rowcount={tracks.length + 1}>
      <div className="track-row track-header" role="row" aria-rowindex={1}>
        <span role="columnheader">Title</span><span role="columnheader">Artist</span>
        <span role="columnheader">Release</span><span role="columnheader">Time</span><span role="columnheader">Actions</span>
      </div>
      <div className="track-viewport" onScroll={onScroll} ref={viewport}>
        <div style={{ height: tracks.length * ROW_HEIGHT, position: "relative" }}>
          <div style={{ transform: `translateY(${first * ROW_HEIGHT}px)` }}>
            {visible.map((track, index) => (
              <div className="track-row" role="row" aria-rowindex={first + index + 2} key={track.id} style={{ height: ROW_HEIGHT }}>
                <strong role="cell">{track.title}</strong>
                <span role="cell">{track.artist_names.join(", ") || "Unknown artist"}</span>
                <span role="cell">{track.release_title ?? "Unknown release"}</span>
                <span role="cell">{duration(track.duration_ms)}</span>
                <div className="track-actions" role="cell">
                  <button type="button" aria-label={`Play ${track.title}`} disabled={!track.playable} onClick={() => onPlay(track)}>▶</button>
                  {onAdd ? (
                    <button type="button" className="secondary" aria-label={`Add ${track.title} to queue`} disabled={!track.playable} onClick={() => onAdd(track)}>＋</button>
                  ) : null}
                </div>
              </div>
            ))}
          </div>
          <div
            aria-hidden="true"
            className="track-sentinel"
            ref={sentinel}
            style={{ position: "absolute", top: Math.max(0, tracks.length * ROW_HEIGHT - 1) }}
          />
        </div>
      </div>
    </div>
  );
}
