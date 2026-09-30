import { useCallback, useEffect, useRef, useState } from "react";

import type { TrackSummary } from "../../contracts";
import { searchLibrary } from "../../lib/ipc";

const PAGE_SIZE = 100;

export function useLibrarySearch(refreshToken: string | null = null) {
  const [query, setQuery] = useState("");
  const [tracks, setTracks] = useState<TrackSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [hasMore, setHasMore] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const request = useRef(0);
  const loadingOffset = useRef<number | null>(null);

  useEffect(() => {
    const requestId = ++request.current;
    loadingOffset.current = null;
    const timer = window.setTimeout(() => {
      setLoading(true);
      setError(null);
      void searchLibrary({ text: query, artist: null, release: null, limit: PAGE_SIZE, offset: 0 })
        .then((results) => {
          if (request.current === requestId) {
            setTracks(results);
            setHasMore(results.length === PAGE_SIZE);
          }
        })
        .catch(() => {
          if (request.current === requestId) setError("Library search is temporarily unavailable.");
        })
        .finally(() => {
          if (request.current === requestId) setLoading(false);
        });
    }, 300);
    return () => window.clearTimeout(timer);
  }, [query, refreshToken]);

  const loadMore = useCallback(async () => {
    const requestId = request.current;
    const offset = tracks.length;
    if (!hasMore || loadingOffset.current === offset) return;
    loadingOffset.current = offset;
    setLoadingMore(true);
    try {
      const results = await searchLibrary({
        text: query,
        artist: null,
        release: null,
        limit: PAGE_SIZE,
        offset,
      });
      if (request.current === requestId) {
        setTracks((current) => {
          const seen = new Set(current.map((track) => track.id));
          return current.concat(results.filter((track) => !seen.has(track.id)));
        });
        setHasMore(results.length === PAGE_SIZE);
      }
    } catch {
      if (request.current === requestId) setError("More library results could not be loaded.");
    } finally {
      if (loadingOffset.current === offset) loadingOffset.current = null;
      if (request.current === requestId) setLoadingMore(false);
    }
  }, [hasMore, query, tracks.length]);

  return { query, setQuery, tracks, loading, loadingMore, hasMore, loadMore, error };
}
