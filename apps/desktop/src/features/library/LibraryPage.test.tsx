import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ComponentType } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import App from "../../App";
import { TrackTable } from "./TrackTable";
import {
  cancelLibraryScan,
  chooseLibraryFolders,
  getAppStatus,
  listenForScanProgress,
  runPlaybackAction,
  searchLibrary,
  startLibraryScan,
} from "../../lib/ipc";

vi.mock("../../lib/ipc", () => ({
  cancelLibraryScan: vi.fn(),
  chooseLibraryFolders: vi.fn(),
  getAppStatus: vi.fn(),
  getPlaybackState: vi.fn(async () => ({
    revision: 0,
    status: "unavailable",
    position_ms: 0,
    duration_ms: null,
    volume: 1,
    muted: false,
    track: null,
    queue: { track_ids: [], current_index: null, can_previous: false, can_next: false },
    external: false,
  })),
  listenForPlaybackState: vi.fn(async () => vi.fn()),
  listenForScanProgress: vi.fn(),
  listenForServiceState: vi.fn(async () => vi.fn()),
  runPlaybackAction: vi.fn(),
  searchLibrary: vi.fn(),
  startLibraryScan: vi.fn(),
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

const ResponsiveTrackTable = TrackTable as unknown as ComponentType<{
  tracks: ReturnType<typeof track>[];
  onPlay: (value: ReturnType<typeof track>) => void;
  hasMore: boolean;
  loadingMore: boolean;
  onLoadMore: () => void;
}>;

let observedHeight = 464;
let resizeCallback: ResizeObserverCallback | undefined;
let intersectionCallback: IntersectionObserverCallback | undefined;
let intersectionIsVisible = false;

class TestResizeObserver implements ResizeObserver {
  constructor(callback: ResizeObserverCallback) {
    resizeCallback = callback;
  }

  observe(target: Element) {
    resizeCallback?.([
      { target, contentRect: { height: observedHeight } as DOMRectReadOnly } as ResizeObserverEntry,
    ], this);
  }

  unobserve() {}
  disconnect() {}
}

class TestIntersectionObserver implements IntersectionObserver {
  readonly root = null;
  readonly rootMargin = "0px";
  readonly thresholds = [0];

  constructor(callback: IntersectionObserverCallback) {
    intersectionCallback = callback;
  }

  observe(target: Element) {
    if (intersectionIsVisible) {
      intersectionCallback?.([
        { target, isIntersecting: true } as IntersectionObserverEntry,
      ], this);
    }
  }

  unobserve() {}
  disconnect() {}
  takeRecords() { return []; }
}

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
  cleanup();
});

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(searchLibrary).mockReset();
  observedHeight = 464;
  resizeCallback = undefined;
  intersectionCallback = undefined;
  intersectionIsVisible = false;
  vi.stubGlobal("ResizeObserver", TestResizeObserver);
  vi.stubGlobal("IntersectionObserver", TestIntersectionObserver);
  localStorage.setItem("fishmuse.onboarding.complete", "true");
  window.history.replaceState({}, "", "#/library");
  vi.mocked(getAppStatus).mockResolvedValue({
    version: "0.1.0",
    database: "ready",
    playback: {
      status: "ready",
      implementation: null,
    },
    ai: { status: "not_configured", implementation: null },
  });
  vi.mocked(listenForScanProgress).mockResolvedValue(vi.fn());
  vi.mocked(runPlaybackAction).mockResolvedValue({ view: null, queue: null });
  vi.mocked(chooseLibraryFolders).mockResolvedValue([]);
  vi.mocked(startLibraryScan).mockResolvedValue({
    scan_id: "01999999-9999-7999-8999-999999999990",
  });
  vi.mocked(cancelLibraryScan).mockResolvedValue(undefined);
});

describe("local library", () => {
  it("prevents overlapping scans, cancels the active ID, and recovers from scan action errors", async () => {
    let publishProgress: Parameters<typeof listenForScanProgress>[0] | undefined;
    vi.mocked(listenForScanProgress).mockImplementation(async (listener) => {
      publishProgress = listener;
      return () => undefined;
    });
    vi.mocked(searchLibrary).mockResolvedValue([]);
    vi.mocked(chooseLibraryFolders)
      .mockRejectedValueOnce(new Error("dialog failed"))
      .mockResolvedValue(["C:\\Music"]);
    vi.mocked(startLibraryScan)
      .mockRejectedValueOnce(new Error("start failed"))
      .mockResolvedValue({ scan_id: "01999999-9999-7999-8999-999999999990" });
    vi.mocked(cancelLibraryScan)
      .mockRejectedValueOnce(new Error("cancel failed"))
      .mockResolvedValue(undefined);

    render(<App />);
    const scan = await screen.findByRole("button", { name: "Scan folders" });

    fireEvent.click(scan);
    expect((await screen.findByRole("alert")).textContent).toMatch(/folder chooser could not be opened/i);

    fireEvent.click(scan);
    expect((await screen.findByRole("alert")).textContent).toMatch(/scan could not be started/i);

    fireEvent.click(scan);
    await waitFor(() => expect(startLibraryScan).toHaveBeenCalledTimes(2));
    expect(screen.queryByRole("alert")).toBeNull();
    expect(scan.hasAttribute("disabled")).toBe(true);
    const cancel = screen.getByRole("button", { name: "Cancel scan" });

    fireEvent.click(cancel);
    expect((await screen.findByRole("alert")).textContent).toMatch(/scan could not be cancelled/i);
    expect(cancelLibraryScan).toHaveBeenLastCalledWith("01999999-9999-7999-8999-999999999990");

    fireEvent.click(cancel);
    await waitFor(() => expect(cancelLibraryScan).toHaveBeenCalledTimes(2));
    expect(screen.queryByRole("alert")).toBeNull();
    expect(scan.hasAttribute("disabled")).toBe(true);

    act(() => publishProgress?.({
      scan_id: "01999999-9999-7999-8999-999999999990",
      discovered: 12,
      parsed: 8,
      unchanged: 4,
      failed: 0,
      status: "cancelled",
      error: null,
    }));
    expect(await screen.findByText("Scan cancelled")).toBeTruthy();
    await waitFor(() => expect(scan.hasAttribute("disabled")).toBe(false));
    expect(screen.queryByRole("button", { name: "Cancel scan" })).toBeNull();

    act(() => publishProgress?.({
      scan_id: "01999999-9999-7999-8999-999999999991",
      discovered: 4,
      parsed: 0,
      unchanged: 0,
      failed: 4,
      status: "failed",
      error: {
        code: "storage_failure",
        category: "library",
        user_message: "The scan stopped safely.",
        retryable: true,
        suggested_action: "retry",
      },
    }));
    expect(await screen.findByText("Scan failed")).toBeTruthy();
    expect(screen.getByText("The scan stopped safely.")).toBeTruthy();
  });

  it("adapts the virtual row window to the measured track viewport", () => {
    observedHeight = 116;
    const tracks = Array.from({ length: 10_000 }, (_, index) =>
      track(`track-${index}`, `Track ${index}`),
    );
    render(
      <ResponsiveTrackTable
        tracks={tracks}
        onPlay={vi.fn()}
        hasMore={false}
        loadingMore={false}
        onLoadMore={vi.fn()}
      />,
    );

    expect(screen.getAllByRole("row")).toHaveLength(11);
    expect(resizeCallback).toBeDefined();

    observedHeight = 580;
    const viewport = document.querySelector<HTMLElement>(".track-viewport");
    if (!viewport || !resizeCallback) throw new Error("observed track viewport missing");
    act(() => resizeCallback?.([
      { target: viewport, contentRect: { height: observedHeight } as DOMRectReadOnly } as unknown as ResizeObserverEntry,
    ], {} as ResizeObserver));

    expect(screen.getAllByRole("row")).toHaveLength(19);
  });

  it("keeps a 10,000-track collection out of the DOM by rendering a virtual window", () => {
    const tracks = Array.from({ length: 10_000 }, (_, index) =>
      track(`track-${index}`, `Track ${index}`),
    );
    render(
      <ResponsiveTrackTable
        tracks={tracks}
        onPlay={vi.fn()}
        hasMore={false}
        loadingMore={false}
        onLoadMore={vi.fn()}
      />,
    );

    expect(screen.getAllByRole("row").length).toBeLessThan(50);
    expect(screen.getByText("Track 0")).toBeTruthy();
    expect(screen.queryByText("Track 9999")).toBeNull();
  });

  it("loads each visible library offset once without an out-of-viewport button", async () => {
    intersectionIsVisible = true;
    const firstPage = Array.from({ length: 100 }, (_, index) => track(`track-${index}`, `Track ${index}`));
    let resolveNext: ((tracks: ReturnType<typeof track>[]) => void) | undefined;
    vi.mocked(searchLibrary)
      .mockResolvedValueOnce(firstPage)
      .mockImplementationOnce(() => new Promise((resolve) => { resolveNext = resolve; }));

    const { container } = render(<App />);
    expect(await screen.findByText("Track 0")).toBeTruthy();

    await waitFor(() => {
      expect(searchLibrary).toHaveBeenLastCalledWith({
        text: "",
        artist: null,
        release: null,
        limit: 100,
        offset: 100,
      });
    });
    expect(screen.queryByRole("button", { name: "Load more tracks" })).toBeNull();
    const sentinel = container.querySelector<HTMLElement>(".track-sentinel");
    if (!sentinel || !intersectionCallback) throw new Error("paging sentinel missing");
    act(() => intersectionCallback?.([
      { target: sentinel, isIntersecting: true } as unknown as IntersectionObserverEntry,
    ], {} as IntersectionObserver));
    expect(searchLibrary).toHaveBeenCalledTimes(2);

    await act(async () => resolveNext?.([track("track-100", "Track 100")]));
    const viewport = container.querySelector<HTMLElement>(".track-viewport");
    expect(viewport).toBeTruthy();
    if (viewport) {
      Object.defineProperty(viewport, "scrollTop", { configurable: true, value: 5_800 });
      fireEvent.scroll(viewport);
    }
    expect(await screen.findByText("Track 100")).toBeTruthy();
  });

  it("renders pagination failures and clears the stale error after a successful retry", async () => {
    intersectionIsVisible = true;
    const firstPage = Array.from({ length: 100 }, (_, index) => track(`track-${index}`, `Track ${index}`));
    vi.mocked(searchLibrary)
      .mockResolvedValueOnce(firstPage)
      .mockRejectedValueOnce(new Error("page failed"))
      .mockResolvedValueOnce([track("track-100", "Track 100")])
      .mockResolvedValue([]);

    const { container } = render(<App />);
    expect((await screen.findByRole("alert")).textContent).toMatch(/more library results could not be loaded/i);
    const sentinel = container.querySelector<HTMLElement>(".track-sentinel");
    if (!sentinel || !intersectionCallback) throw new Error("paging sentinel missing");
    act(() => intersectionCallback?.([
      { target: sentinel, isIntersecting: true } as unknown as IntersectionObserverEntry,
    ], {} as IntersectionObserver));

    await waitFor(() => {
      expect(screen.getByRole("table", { name: "Library tracks" }).getAttribute("aria-rowcount")).toBe("102");
    });
    await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
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
      expect(runPlaybackAction).toHaveBeenCalledWith({
        kind: "playNow",
        trackId: "01999999-9999-7999-8999-999999999981",
        context: ["01999999-9999-7999-8999-999999999981"],
      });
    });
    expect(JSON.stringify(vi.mocked(runPlaybackAction).mock.calls)).not.toMatch(/[A-Z]:\\/);

    fireEvent.click(screen.getByRole("button", { name: "Add River Song to queue" }));
    await waitFor(() => {
      expect(runPlaybackAction).toHaveBeenLastCalledWith({
        kind: "add",
        trackId: "01999999-9999-7999-8999-999999999981",
      });
    });
    expect(vi.mocked(runPlaybackAction).mock.calls.filter(([action]) => action.kind === "playNow")).toHaveLength(1);
  });

  it("reports automatic playback startup failure without naming its implementation", async () => {
    vi.mocked(searchLibrary).mockResolvedValue([
      track("01999999-9999-7999-8999-999999999981", "River Song"),
    ]);
    vi.mocked(runPlaybackAction)
      .mockRejectedValueOnce(new Error("startup failed"))
      .mockResolvedValue({ view: null, queue: null });

    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "Play River Song" }));

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toMatch(/playback service is unavailable/i);
    expect(alert.textContent?.toLowerCase()).not.toContain("foobar");

    fireEvent.click(screen.getByRole("button", { name: "Play River Song" }));
    await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
  });

  it("plays with the current ordered playable context and never includes disabled tracks", async () => {
    vi.mocked(searchLibrary).mockResolvedValue([
      track("01999999-9999-7999-8999-999999999981", "First"),
      { ...track("01999999-9999-7999-8999-999999999982", "Unavailable"), playable: false },
      track("01999999-9999-7999-8999-999999999983", "Third"),
    ]);
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Play Third" }));
    await waitFor(() => {
      expect(runPlaybackAction).toHaveBeenCalledWith({
        kind: "playNow",
        trackId: "01999999-9999-7999-8999-999999999983",
        context: [
          "01999999-9999-7999-8999-999999999981",
          "01999999-9999-7999-8999-999999999983",
        ],
      });
    });
  });

  it("refreshes the current query when a scan completes", async () => {
    vi.useFakeTimers();
    let publishProgress: Parameters<typeof listenForScanProgress>[0] | undefined;
    vi.mocked(listenForScanProgress).mockImplementation(async (listener) => {
      publishProgress = listener;
      return () => undefined;
    });
    vi.mocked(searchLibrary)
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce([
        track("01999999-9999-7999-8999-999999999981", "Newly Scanned"),
      ]);

    render(<App />);
    await act(async () => { await vi.advanceTimersByTimeAsync(300); });
    expect(screen.getByText("No tracks found")).toBeTruthy();

    act(() => publishProgress?.({
      scan_id: "01999999-9999-7999-8999-999999999990",
      discovered: 1,
      parsed: 1,
      unchanged: 0,
      failed: 0,
      status: "completed",
      error: null,
    }));
    await act(async () => { await vi.advanceTimersByTimeAsync(300); });

    expect(screen.getByText("Newly Scanned")).toBeTruthy();
    expect(searchLibrary).toHaveBeenCalledTimes(2);
  });
});
