import type {
  PlaybackActionResult,
  PlaybackView,
  QueueSnapshot,
} from "../contracts";

type PlaybackStoreState = {
  snapshot: PlaybackView;
  receivedAt: number;
};

const emptyQueue = (): QueueSnapshot => ({
  track_ids: [],
  current_index: null,
  can_previous: false,
  can_next: false,
});

const unavailableSnapshot = (): PlaybackView => ({
  revision: 0,
  status: "unavailable",
  position_ms: 0,
  duration_ms: null,
  volume: 1,
  muted: false,
  track: null,
  queue: emptyQueue(),
  external: false,
});

export function createPlaybackStore() {
  let state: PlaybackStoreState = { snapshot: unavailableSnapshot(), receivedAt: Date.now() };
  let hasSnapshot = false;
  let lastNonZeroVolume = 1;
  const listeners = new Set<() => void>();

  const notify = () => listeners.forEach((listener) => listener());
  const project = (at = Date.now()): PlaybackView => {
    const { snapshot, receivedAt } = state;
    if (snapshot.status !== "playing") return snapshot;
    const elapsed = Math.max(0, at - receivedAt);
    const position = snapshot.position_ms + elapsed;
    return {
      ...snapshot,
      position_ms: snapshot.duration_ms == null ? position : Math.min(position, snapshot.duration_ms),
    };
  };
  const acceptQueue = (queue: QueueSnapshot) => {
    state = {
      ...state,
      snapshot: { ...state.snapshot, queue },
    };
    notify();
  };

  return {
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    getSnapshot() {
      return state;
    },
    accept(next: PlaybackView, receivedAt = Date.now()) {
      if (hasSnapshot && next.revision <= state.snapshot.revision) return;
      const lastKnown = project(receivedAt);
      const retained = next.status === "unavailable" && state.snapshot.track
        ? {
            ...next,
            track: lastKnown.track,
            external: lastKnown.external,
            position_ms: lastKnown.position_ms,
            duration_ms: lastKnown.duration_ms,
          }
        : next;
      if (retained.volume > 0) lastNonZeroVolume = retained.volume;
      state = { snapshot: retained, receivedAt };
      hasSnapshot = true;
      notify();
    },
    acceptQueue,
    acceptActionResult(result: PlaybackActionResult) {
      if (result.view) this.accept(result.view);
      if (result.queue) acceptQueue(result.queue);
    },
    project,
    lastNonZeroVolume() {
      return lastNonZeroVolume;
    },
    reset() {
      state = { snapshot: unavailableSnapshot(), receivedAt: Date.now() };
      hasSnapshot = false;
      lastNonZeroVolume = 1;
      notify();
    },
  };
}

export const playbackStore = createPlaybackStore();

export function resetPlaybackState() {
  playbackStore.reset();
}
