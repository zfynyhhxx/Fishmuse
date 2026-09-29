import { useEffect, useState, useSyncExternalStore } from "react";

import { playbackStore } from "../../state/playbackStore";

export function usePlaybackState() {
  const base = useSyncExternalStore(playbackStore.subscribe, playbackStore.getSnapshot);
  const [clock, setClock] = useState(() => Date.now());

  useEffect(() => {
    if (base.snapshot.status !== "playing") return undefined;
    const timer = window.setInterval(() => setClock(Date.now()), 500);
    return () => window.clearInterval(timer);
  }, [base]);

  return playbackStore.project(clock);
}
