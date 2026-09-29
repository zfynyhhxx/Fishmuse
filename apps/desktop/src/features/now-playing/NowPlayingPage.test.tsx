import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { executePlayback } from "../../lib/ipc";
import {
  createPlaybackStore,
  playbackStore,
  resetPlaybackState,
} from "../../state/playbackStore";
import { NowPlayingPage } from "./NowPlayingPage";

vi.mock("../../lib/ipc", () => ({
  executePlayback: vi.fn(),
}));

const TRACK_ID = "01999999-9999-7999-8999-999999999981";

beforeEach(() => {
  vi.clearAllMocks();
  resetPlaybackState();
});

afterEach(cleanup);

describe("revisioned playback state", () => {
  it("interpolates playing position, calibrates on a newer snapshot, ignores stale revisions, and retains the track while unavailable", () => {
    const store = createPlaybackStore();
    store.accept({
      revision: 4,
      status: "playing",
      track_id: TRACK_ID,
      position_ms: 1_000,
      duration_ms: 5_000,
    }, 10_000);

    expect(store.project(12_000).position_ms).toBe(3_000);
    store.accept({
      revision: 3,
      status: "paused",
      track_id: TRACK_ID,
      position_ms: 500,
      duration_ms: 5_000,
    }, 12_000);
    expect(store.project(12_000).status).toBe("playing");

    store.accept({
      revision: 5,
      status: "paused",
      track_id: TRACK_ID,
      position_ms: 2_500,
      duration_ms: 5_000,
    }, 12_000);
    expect(store.project(20_000).position_ms).toBe(2_500);

    store.accept({
      revision: 6,
      status: "unavailable",
      track_id: null,
      position_ms: 0,
      duration_ms: null,
    }, 13_000);
    expect(store.project(13_000)).toMatchObject({
      status: "unavailable",
      track_id: TRACK_ID,
      position_ms: 2_500,
      duration_ms: 5_000,
    });

    store.accept({
      revision: 7,
      status: "stopped",
      track_id: null,
      position_ms: 0,
      duration_ms: null,
    }, 14_000);
    expect(store.project(14_000).status).toBe("stopped");
  });

  it("deduplicates rapid controls and assigns a fresh UUIDv7 operation to pause, resume, seek, and next", async () => {
    playbackStore.accept({
      revision: 1,
      status: "playing",
      track_id: TRACK_ID,
      position_ms: 10_000,
      duration_ms: 120_000,
    });
    let resolvePause: ((value: ReturnType<typeof playbackStore.project>) => void) | undefined;
    vi.mocked(executePlayback)
      .mockImplementationOnce(() => new Promise((resolve) => { resolvePause = resolve; }))
      .mockResolvedValueOnce({ revision: 3, status: "playing", track_id: TRACK_ID, position_ms: 10_000, duration_ms: 120_000 })
      .mockResolvedValueOnce({ revision: 4, status: "playing", track_id: TRACK_ID, position_ms: 42_000, duration_ms: 120_000 })
      .mockResolvedValueOnce({ revision: 5, status: "playing", track_id: TRACK_ID, position_ms: 0, duration_ms: 180_000 });

    render(<NowPlayingPage />);
    const pause = screen.getByRole("button", { name: "Pause" });
    fireEvent.click(pause);
    fireEvent.click(pause);
    expect(executePlayback).toHaveBeenCalledTimes(1);

    await act(async () => {
      resolvePause?.({ revision: 2, status: "paused", track_id: TRACK_ID, position_ms: 10_000, duration_ms: 120_000 });
    });
    fireEvent.click(await screen.findByRole("button", { name: "Resume" }));
    await waitFor(() => expect(executePlayback).toHaveBeenCalledTimes(2));

    fireEvent.change(screen.getByRole("slider", { name: "Seek position" }), { target: { value: "42000" } });
    fireEvent.click(screen.getByRole("button", { name: "Seek" }));
    await waitFor(() => expect(executePlayback).toHaveBeenCalledTimes(3));

    fireEvent.click(screen.getByRole("button", { name: "Next track" }));
    await waitFor(() => expect(executePlayback).toHaveBeenCalledTimes(4));

    const commands = vi.mocked(executePlayback).mock.calls.map(([command]) => command);
    expect(commands.map((command) => command.kind)).toEqual(["pause", "resume", "seek", "skip_next"]);
    expect(new Set(commands.map((command) => command.operation_id)).size).toBe(4);
    for (const command of commands) {
      expect(command.operation_id).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i);
    }
  });
});
