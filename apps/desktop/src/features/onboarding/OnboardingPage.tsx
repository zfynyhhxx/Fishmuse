import { useState } from "react";

import { chooseLibraryFolders, startLibraryScan } from "../../lib/ipc";
import { useAppStore } from "../../state/appStore";

export function OnboardingPage() {
  const { scanProgress } = useAppStore();
  const [scanId, setScanId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const chooseFolders = async () => {
    setBusy(true);
    setError(null);
    try {
      const roots = await chooseLibraryFolders();
      if (roots.length === 0) return;
      const started = await startLibraryScan(roots);
      localStorage.setItem("fishmuse.onboarding.complete", "true");
      setScanId(started.scan_id);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "FishMuse could not start the scan.");
    } finally {
      setBusy(false);
    }
  };

  const current = scanProgress?.scan_id === scanId ? scanProgress : null;
  return (
    <section className="hero-card" aria-labelledby="onboarding-title">
      <p className="eyebrow">Welcome to FishMuse</p>
      <h1 id="onboarding-title">Your music stays yours</h1>
      <p>FishMuse scans your folders locally. Your audio and file paths stay on this computer.</p>
      <div className="button-row">
        <button type="button" onClick={() => void chooseFolders()} disabled={busy}>
          {busy ? "Opening folders…" : "Choose music folders"}
        </button>
        {scanId ? <a className="button secondary" href="#/library">Open Library</a> : null}
      </div>
      {current ? (
        <p role="status">{current.parsed} of {current.discovered} tracks inspected · {current.failed} need attention</p>
      ) : null}
      {error ? <p className="error-banner" role="alert">{error}</p> : null}
    </section>
  );
}
