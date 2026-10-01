import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { PlaybackView } from "../../contracts";
import { MiniPlayer } from "../../components/MiniPlayer";
import { getTrackArtwork, retryPlaybackService, runPlaybackAction } from "../../lib/ipc";
import {
  createPlaybackStore,
  playbackStore,
  resetPlaybackState,
} from "../../state/playbackStore";
import { NowPlayingPage } from "./NowPlayingPage";

vi.mock("../../lib/ipc", () => ({
  getTrackArtwork: vi.fn(),
  retryPlaybackService: vi.fn(),
  runPlaybackAction: vi.fn(),
}));

const TRACK_ID = "01999999-9999-7999-8999-999999999981";
const NEXT_ID = "01999999-9999-7999-8999-999999999982";

const view = (overrides: Partial<PlaybackView> = {}): PlaybackView => ({
  revision: 1,
  status: "playing",
  position_ms: 10_000,
  duration_ms: 120_000,
  volume: 0.5,
  muted: false,
  track: {
    id: TRACK_ID,
    title: "River Song",
    artist_names: ["Fish Artist"],
    release_title: "Local Waters",
    artwork_available: true,
  },
  queue: {
    track_ids: [TRACK_ID, NEXT_ID],
    current_index: 0,
    can_previous: true,
    can_next: true,
  },
  external: false,
  ...overrides,
});

beforeEach(() => {
  vi.clearAllMocks();
  resetPlaybackState();
  vi.mocked(getTrackArtwork).mockResolvedValue({ data_url: "data:image/png;base64,cGljdHVyZQ==" });
  vi.mocked(runPlaybackAction).mockResolvedValue({ view: null, queue: null });
  vi.mocked(retryPlaybackService).mockResolvedValue(undefined);
});

afterEach(cleanup);

describe("revisioned playback state", () => {
  it("interpolates position, ignores stale revisions, and retains safe metadata while unavailable", () => {
    const store = createPlaybackStore();
    store.accept(view({ revision: 4, position_ms: 1_000 }), 10_000);
    expect(store.project(12_000).position_ms).toBe(3_000);

    store.accept(view({ revision: 3, status: "paused", position_ms: 500 }), 12_000);
    expect(store.project(12_000).status).toBe("playing");

    store.accept(view({ revision: 5, status: "paused", position_ms: 2_500 }), 12_000);
    expect(store.project(20_000).position_ms).toBe(2_500);

    store.accept(view({
      revision: 6,
      status: "unavailable",
      position_ms: 0,
      duration_ms: null,
      track: null,
    }), 13_000);
    expect(store.project(13_000)).toMatchObject({
      status: "unavailable",
      position_ms: 2_500,
      duration_ms: 120_000,
      track: { title: "River Song" },
    });
  });
});

describe("safe playback display", () => {
  it("renders title, artists, release, and bounded artwork without exposing the track UUID", async () => {
    playbackStore.accept(view());
    render(<NowPlayingPage />);

    expect(screen.getByRole("heading", { name: "River Song" })).toBeTruthy();
    expect(screen.getByText("Fish Artist")).toBeTruthy();
    expect(screen.getByText("Local Waters")).toBeTruthy();
    expect(screen.queryByText(TRACK_ID)).toBeNull();
    const artwork = await screen.findByRole("img", { name: "Artwork for River Song" });
    expect(artwork.getAttribute("src")).toBe("data:image/png;base64,cGljdHVyZQ==");
    expect(getTrackArtwork).toHaveBeenCalledWith(TRACK_ID);
  });

  it("renders deterministic unknown, placeholder, and external playback states", () => {
    playbackStore.accept(view({
      track: {
        id: TRACK_ID,
        title: "Untitled current track",
        artist_names: [],
        release_title: null,
        artwork_available: false,
      },
    }));
    const first = render(<NowPlayingPage />);
    expect(screen.getByText("Unknown artist")).toBeTruthy();
    expect(screen.getByText("Unknown release")).toBeTruthy();
    expect(screen.getByRole("img", { name: "No artwork available" })).toBeTruthy();
    first.unmount();

    resetPlaybackState();
    playbackStore.accept(view({ revision: 2, track: null, external: true }));
    render(<NowPlayingPage />);
    expect(screen.getByRole("heading", { name: "External playback" })).toBeTruthy();
  });
});

describe("complete playback controls", () => {
  it("gives the MiniPlayer Previous, Play-Pause, and Next controls", () => {
    playbackStore.accept(view());
    render(<MiniPlayer />);
    expect(screen.getByRole("button", { name: "Previous track" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Pause" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Next track" })).toBeTruthy();
  });

  it("deduplicates rapid controls and exposes stop, seek, volume, mute, and queue actions", async () => {
    playbackStore.accept(view());
    let resolvePause: ((value: { view: PlaybackView | null; queue: null }) => void) | undefined;
    vi.mocked(runPlaybackAction).mockImplementationOnce(() => new Promise((resolve) => {
      resolvePause = resolve;
    }));
    render(<NowPlayingPage />);

    const pause = screen.getByRole("button", { name: "Pause" });
    fireEvent.click(pause);
    fireEvent.click(pause);
    expect(runPlaybackAction).toHaveBeenCalledTimes(1);
    await act(async () => resolvePause?.({ view: view({ revision: 2, status: "paused" }), queue: null }));

    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    fireEvent.change(screen.getByRole("slider", { name: "Seek position" }), { target: { value: "42000" } });
    fireEvent.click(screen.getByRole("button", { name: "Seek" }));
    fireEvent.change(screen.getByRole("slider", { name: "Volume" }), { target: { value: "75" } });
    fireEvent.click(screen.getByRole("button", { name: "Set volume" }));
    await waitFor(() => expect(runPlaybackAction).toHaveBeenCalledTimes(4));
    fireEvent.click(screen.getByRole("button", { name: "Mute" }));
    fireEvent.click(screen.getByRole("button", { name: "Play queued track 2" }));
    fireEvent.click(screen.getByRole("button", { name: "Remove queued track 2" }));
    fireEvent.click(screen.getByRole("button", { name: "Clear queue" }));

    await waitFor(() => expect(runPlaybackAction).toHaveBeenCalledTimes(8));
    expect(vi.mocked(runPlaybackAction).mock.calls.map(([action]) => action)).toEqual([
      { kind: "pause" },
      { kind: "stop" },
      { kind: "seek", positionMs: 42_000 },
      { kind: "setVolume", volume: 0.75 },
      { kind: "setVolume", volume: 0 },
      { kind: "playAt", index: 1 },
      { kind: "remove", index: 1 },
      { kind: "clear" },
    ]);
  });

  it("clears a generic playback error after a successful retry", async () => {
    playbackStore.accept(view());
    vi.mocked(runPlaybackAction).mockRejectedValueOnce(new Error("private backend details"));
    render(<NowPlayingPage />);

    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    expect((await screen.findByRole("alert")).textContent).toMatch(/playback control is unavailable/i);
    expect(screen.getByRole("alert").textContent?.toLowerCase()).not.toContain("foobar");
    expect(screen.getByRole("link", { name: "Advanced diagnostics" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Retry playback" }));
    await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
    expect(retryPlaybackService).toHaveBeenCalledOnce();
  });

  it("shows only an allow-listed structured playback message", async () => {
    playbackStore.accept(view());
    vi.mocked(runPlaybackAction).mockRejectedValueOnce({
      code: "backend_unavailable",
      category: "playback",
      user_message: "The selected track is unavailable.",
      retryable: true,
      suggested_action: "refresh_playback_state",
      technical_context: "C:\\private\\music.flac foobar",
    });
    render(<NowPlayingPage />);

    fireEvent.click(screen.getByRole("button", { name: "Pause" }));
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("The selected track is unavailable.");
    expect(alert.textContent).not.toContain("C:\\private");
    expect(alert.textContent?.toLowerCase()).not.toContain("foobar");
  });

  it("replaces internal code-style messages with generic recovery copy", async () => {
    playbackStore.accept(view());
    vi.mocked(runPlaybackAction).mockRejectedValueOnce({
      code: "storage_failure",
      category: "storage",
      user_message: "storage_failure",
      retryable: false,
      suggested_action: null,
    });
    render(<NowPlayingPage />);

    fireEvent.click(screen.getByRole("button", { name: "Pause" }));
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toMatch(/playback control is unavailable/i);
    expect(alert.textContent).not.toContain("storage_failure");
  });

  it("rejects backend-named structured messages and actions at the UI boundary", async () => {
    playbackStore.accept(view());
    vi.mocked(runPlaybackAction).mockRejectedValueOnce({
      code: "backend_unavailable",
      category: "playback",
      user_message: "Restart foobar2000 at C:\\private\\music.flac.",
      retryable: true,
      suggested_action: "restart_foobar2000",
    });
    render(<NowPlayingPage />);

    fireEvent.click(screen.getByRole("button", { name: "Pause" }));
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toMatch(/playback control is unavailable/i);
    expect(alert.textContent?.toLowerCase()).not.toContain("foobar");
    expect(alert.textContent).not.toContain("C:\\private");
  });
});
