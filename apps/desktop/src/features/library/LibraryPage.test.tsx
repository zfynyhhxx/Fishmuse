import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import App from "../../App";
import { TrackTable } from "./TrackTable";
import {
  executePlayback,
  getAppStatus,
  listenForScanProgress,
  searchLibrary,
} from "../../lib/ipc";

vi.mock("../../lib/ipc", () => ({
  executePlayback: vi.fn(),
  getAppStatus: vi.fn(),
  getPlaybackState: vi.fn(async () => ({ revision: 0, status: "unavailable", track_id: null, position_ms: 0, duration_ms: null })),
  listenForPlaybackState: vi.fn(async () => vi.fn()),
  listenForScanProgress: vi.fn(),
  listenForServiceState: vi.fn(async () => vi.fn()),
  searchLibrary: vi.fn(),
}));

const track = (id: string, title: string) => ({
  id,
  recording_id: "01999999-9999-7999-8999-999999999980",
  title,
  artist_names: ["Fish Artist"],
  release_title: "Local Waters",
  duration_ms: 180_000,
  disc_number: 1,
  track_number: 1,
  playable: true,
});

afterEach(() => {
  vi.useRealTimers();
  cleanup();
});

beforeEach(() => {
  localStorage.setItem("fishmuse.onboarding.complete", "true");
  window.history.replaceState({}, "", "#/library");
  vi.mocked(getAppStatus).mockResolvedValue({
    version: "0.1.0",
    database: "ready",
    playback: {
      status: "ready",
      implementation: { id: "foobar2000", display_name: "foobar2000" },
    },
    ai: { status: "not_configured", implementation: null },
  });
  vi.mocked(listenForScanProgress).mockResolvedValue(vi.fn());
  vi.mocked(executePlayback).mockResolvedValue({
    revision: 1,
    status: "playing",
    track_id: "01999999-9999-7999-8999-999999999981",
    position_ms: 0,
    duration_ms: 180_000,
  });
});

describe("local library", () => {
  it("keeps a 10,000-track collection out of the DOM by rendering a virtual window", () => {
    const tracks = Array.from({ length: 10_000 }, (_, index) =>
      track(`track-${index}`, `Track ${index}`),
    );
    render(<TrackTable tracks={tracks} onPlay={vi.fn()} />);

    expect(screen.getAllByRole("row").length).toBeLessThan(50);
    expect(screen.getByText("Track 0")).toBeTruthy();
    expect(screen.queryByText("Track 9999")).toBeNull();
  });

  it("loads user-visible library pages beyond the AI tool result limit", async () => {
    const firstPage = Array.from({ length: 100 }, (_, index) => track(`track-${index}`, `Track ${index}`));
    vi.mocked(searchLibrary)
      .mockResolvedValueOnce(firstPage)
      .mockResolvedValueOnce([track("track-100", "Track 100")]);

    const { container } = render(<App />);
    expect(await screen.findByText("Track 0")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Load more tracks" }));

    await waitFor(() => {
      expect(searchLibrary).toHaveBeenLastCalledWith({
        text: "",
        artist: null,
        release: null,
        limit: 100,
        offset: 100,
      });
    });
    const viewport = container.querySelector<HTMLElement>(".track-viewport");
    expect(viewport).toBeTruthy();
    if (viewport) {
      Object.defineProperty(viewport, "scrollTop", { configurable: true, value: 5_800 });
      fireEvent.scroll(viewport);
    }
    expect(await screen.findByText("Track 100")).toBeTruthy();
  });

  it("debounces search and prevents an older response from replacing newer results", async () => {
    vi.useFakeTimers();
    let resolveOld: ((value: ReturnType<typeof track>[]) => void) | undefined;
    vi.mocked(searchLibrary)
      .mockResolvedValueOnce([])
      .mockImplementationOnce(() => new Promise((resolve) => { resolveOld = resolve; }))
      .mockResolvedValueOnce([track("01999999-9999-7999-8999-999999999981", "New Current")]);

    render(<App />);
    await act(async () => { await vi.runAllTimersAsync(); });

    const input = screen.getByRole("searchbox", { name: "Search your library" });
    fireEvent.change(input, { target: { value: "old" } });
    await act(async () => { await vi.advanceTimersByTimeAsync(300); });
    fireEvent.change(input, { target: { value: "new" } });
    await act(async () => { await vi.advanceTimersByTimeAsync(300); });

    expect(screen.getByText("New Current")).toBeTruthy();
    await act(async () => { resolveOld?.([track("01999999-9999-7999-8999-999999999982", "Old Stale")]); });
    expect(screen.queryByText("Old Stale")).toBeNull();
    expect(vi.mocked(searchLibrary).mock.calls.at(-1)?.[0].limit).toBeGreaterThan(20);
  });

  it("shows scan diagnostics without blocking navigation and sends a logical play command", async () => {
    let publishProgress: Parameters<typeof listenForScanProgress>[0] | undefined;
    vi.mocked(listenForScanProgress).mockImplementation(async (listener) => {
      publishProgress = listener;
      return () => undefined;
    });
    vi.mocked(searchLibrary).mockResolvedValue([
      track("01999999-9999-7999-8999-999999999981", "River Song"),
    ]);

    render(<App />);
    expect(await screen.findByText("River Song")).toBeTruthy();

    act(() => publishProgress?.({
      scan_id: "01999999-9999-7999-8999-999999999990",
      discovered: 40,
      parsed: 32,
      unchanged: 3,
      failed: 5,
      status: "running",
      error: {
        code: "invalid_tags",
        category: "library",
        user_message: "Five files could not be read.",
        retryable: false,
        suggested_action: "review_diagnostics",
      },
    }));

    expect(await screen.findByText("5 files need attention")).toBeTruthy();
    expect(screen.getByRole("link", { name: "Settings" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Play River Song" }));

    await waitFor(() => {
      expect(executePlayback).toHaveBeenCalledWith({
        kind: "play",
        track_id: "01999999-9999-7999-8999-999999999981",
        operation_id: expect.stringMatching(/^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i),
      });
    });
    expect(JSON.stringify(vi.mocked(executePlayback).mock.calls)).not.toMatch(/[A-Z]:\\/);
  });
});
