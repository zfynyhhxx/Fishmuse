import { useState } from "react";

import type { TrackSummary } from "../../contracts";
import { chooseLibraryFolders, executePlayback, startLibraryScan } from "../../lib/ipc";
import { newUuidV7 } from "../../lib/uuid";
import { useAppStore } from "../../state/appStore";
import { playbackStore } from "../../state/playbackStore";
import { TrackTable } from "./TrackTable";
import { useLibrarySearch } from "./useLibrarySearch";

export function LibraryPage() {
  const { query, setQuery, tracks, loading, loadingMore, hasMore, loadMore, error } = useLibrarySearch();
  const { scanProgress } = useAppStore();
  const [actionError, setActionError] = useState<string | null>(null);

  const scan = async () => {
    const roots = await chooseLibraryFolders();
    if (roots.length > 0) await startLibraryScan(roots);
  };
  const play = async (track: TrackSummary) => {
    try {
      playbackStore.accept(await executePlayback({ kind: "play", track_id: track.id, operation_id: newUuidV7() }));
    } catch {
      setActionError("Playback is unavailable. Start foobar2000 from Settings.");
    }
  };

  return (
    <section className="page-stack" aria-labelledby="library-title">
      <div className="page-heading">
        <div><p className="eyebrow">On this computer</p><h1 id="library-title">Library</h1></div>
        <button type="button" onClick={() => void scan()}>Scan folders</button>
      </div>
      {scanProgress ? (
        <section className="scan-card" aria-label="Library scan progress">
          <strong>{scanProgress.status === "running" ? "Scanning in the background" : "Scan finished"}</strong>
          <span>{scanProgress.parsed} parsed · {scanProgress.unchanged} unchanged</span>
          {scanProgress.failed > 0 ? (
            <details>
              <summary>{scanProgress.failed} files need attention</summary>
              <p>{scanProgress.error?.user_message ?? "Some files could not be read. Your other music is still available."}</p>
            </details>
          ) : null}
        </section>
      ) : null}
      <label className="search-field">
        <span>Search your library</span>
        <input type="search" value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Title, artist, or release" />
      </label>
      {error || actionError ? <p className="error-banner" role="alert">{error ?? actionError}</p> : null}
      {loading && tracks.length === 0 ? <p role="status">Searching your library…</p> : null}
      {!loading && tracks.length === 0 ? (
        <div className="empty-state"><h2>No tracks found</h2><p>Try another search or scan a music folder.</p></div>
      ) : null}
      {tracks.length > 0 ? <TrackTable tracks={tracks} onPlay={(track) => void play(track)} /> : null}
      {hasMore ? (
        <button type="button" disabled={loadingMore} onClick={() => void loadMore()}>
          {loadingMore ? "Loading more…" : "Load more tracks"}
        </button>
      ) : null}
    </section>
  );
}
