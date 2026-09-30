import { useState } from "react";

import type { TrackSummary } from "../../contracts";
import {
  cancelLibraryScan,
  chooseLibraryFolders,
  runPlaybackAction,
  startLibraryScan,
} from "../../lib/ipc";
import { useAppStore } from "../../state/appStore";
import { playbackStore } from "../../state/playbackStore";
import { TrackTable } from "./TrackTable";
import { useLibrarySearch } from "./useLibrarySearch";

export function LibraryPage() {
  const { scanProgress } = useAppStore();
  const completedScan = scanProgress && scanProgress.status !== "running"
    ? `${scanProgress.scan_id}:${scanProgress.status}`
    : null;
  const { query, setQuery, tracks, loading, loadingMore, hasMore, loadMore, error } = useLibrarySearch(completedScan);
  const [actionError, setActionError] = useState<string | null>(null);
  const [pendingScanId, setPendingScanId] = useState<string | null>(null);
  const runningScanId = scanProgress?.status === "running" ? scanProgress.scan_id : null;
  const pendingScanFinished = pendingScanId != null
    && scanProgress?.scan_id === pendingScanId
    && scanProgress.status !== "running";
  const activeScanId = runningScanId ?? (pendingScanFinished ? null : pendingScanId);
  const scanBusy = activeScanId != null;

  const scan = async () => {
    if (scanBusy) return;
    setActionError(null);
    let roots: string[];
    try {
      roots = await chooseLibraryFolders();
    } catch {
      setActionError("The folder chooser could not be opened. Please try again.");
      return;
    }
    if (roots.length === 0) return;
    try {
      const started = await startLibraryScan(roots);
      setPendingScanId(started.scan_id);
    } catch {
      setActionError("The library scan could not be started. Please try again.");
    }
  };
  const cancelScan = async () => {
    if (!activeScanId) return;
    setActionError(null);
    try {
      await cancelLibraryScan(activeScanId);
    } catch {
      setActionError("The library scan could not be cancelled. It may have already finished.");
    }
  };
  const play = async (track: TrackSummary) => {
    setActionError(null);
    try {
      playbackStore.acceptActionResult(await runPlaybackAction({
        kind: "playNow",
        trackId: track.id,
        context: tracks.filter((candidate) => candidate.playable).map((candidate) => candidate.id),
      }));
    } catch {
      setActionError("The playback service is unavailable. Retry from advanced diagnostics in Settings.");
    }
  };
  const add = async (track: TrackSummary) => {
    setActionError(null);
    try {
      playbackStore.acceptActionResult(await runPlaybackAction({ kind: "add", trackId: track.id }));
    } catch {
      setActionError("The playback service is unavailable. Retry from advanced diagnostics in Settings.");
    }
  };

  return (
    <section className="library-page" aria-labelledby="library-title">
      <div className="library-chrome">
        <div className="page-heading">
          <div><p className="eyebrow">On this computer</p><h1 id="library-title">Library</h1></div>
          <div className="button-row">
            <button type="button" disabled={scanBusy} onClick={() => void scan()}>Scan folders</button>
            {activeScanId ? (
              <button type="button" className="secondary" onClick={() => void cancelScan()}>Cancel scan</button>
            ) : null}
          </div>
        </div>
        {scanProgress ? (
          <section className="scan-card" aria-label="Library scan progress">
            <strong>{({
              running: "Scanning in the background",
              completed: "Scan complete",
              cancelled: "Scan cancelled",
              failed: "Scan failed",
            } as const)[scanProgress.status]}</strong>
            <span>{scanProgress.parsed} parsed · {scanProgress.unchanged} unchanged</span>
            {scanProgress.error && scanProgress.failed === 0 ? <p>{scanProgress.error.user_message}</p> : null}
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
      </div>
      <div className="library-results">
        {loading && tracks.length === 0 ? <p role="status">Searching your library…</p> : null}
        {!loading && tracks.length === 0 ? (
          <div className="empty-state"><h2>No tracks found</h2><p>Try another search or scan a music folder.</p></div>
        ) : null}
        {tracks.length > 0 ? (
          <TrackTable
            tracks={tracks}
            onPlay={(track) => void play(track)}
            onAdd={(track) => void add(track)}
            hasMore={hasMore}
            loadingMore={loadingMore}
            onLoadMore={() => void loadMore()}
          />
        ) : null}
      </div>
    </section>
  );
}
