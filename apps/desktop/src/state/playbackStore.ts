import type { PlaybackSnapshot } from "../contracts";

type PlaybackStoreState = {
  snapshot: PlaybackSnapshot;
  receivedAt: number;
};

const unavailableSnapshot: PlaybackSnapshot = {
  revision: 0,
  status: "unavailable",
  track_id: null,
  position_ms: 0,
  duration_ms: null,
};

export function createPlaybackStore() {
  let state: PlaybackStoreState = { snapshot: unavailableSnapshot, receivedAt: Date.now() };
  let hasSnapshot = false;
  const listeners = new Set<() => void>();

  const notify = () => listeners.forEach((listener) => listener());
  const project = (at = Date.now()): PlaybackSnapshot => {
    const { snapshot, receivedAt } = state;
    if (snapshot.status !== "playing") return snapshot;
    const elapsed = Math.max(0, at - receivedAt);
    const position = snapshot.position_ms + elapsed;
    return {
      ...snapshot,
      position_ms: snapshot.duration_ms == null ? position : Math.min(position, snapshot.duration_ms),
    };
  };
  return {
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    getSnapshot() {
      return state;
    },
    accept(next: PlaybackSnapshot, receivedAt = Date.now()) {
      if (hasSnapshot && next.revision <= state.snapshot.revision) return;
      const lastKnown = project(receivedAt);
      const retained = next.status === "unavailable" && state.snapshot.track_id
        ? {
            ...next,
            track_id: lastKnown.track_id,
            position_ms: lastKnown.position_ms,
            duration_ms: lastKnown.duration_ms,
          }
        : next;
      state = { snapshot: retained, receivedAt };
      hasSnapshot = true;
      notify();
    },
    project,
    reset() {
      state = { snapshot: unavailableSnapshot, receivedAt: Date.now() };
      hasSnapshot = false;
      notify();
    },
  };
}

export const playbackStore = createPlaybackStore();

export function resetPlaybackState() {
  playbackStore.reset();
}
