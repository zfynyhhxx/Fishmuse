import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import App from "./App";
import { getAppStatus, retryPlaybackService } from "./lib/ipc";

vi.mock("./lib/ipc", () => ({
  getAppStatus: vi.fn(),
  retryPlaybackService: vi.fn(),
  getPlaybackState: vi.fn(async () => ({ revision: 0, status: "unavailable", track_id: null, position_ms: 0, duration_ms: null })),
  listenForPlaybackState: vi.fn(async () => vi.fn()),
  listenForScanProgress: vi.fn(async () => vi.fn()),
  listenForServiceState: vi.fn(async () => vi.fn()),
  searchLibrary: vi.fn(async () => []),
}));

const mockedGetAppStatus = vi.mocked(getAppStatus);

afterEach(cleanup);

beforeEach(() => {
  localStorage.clear();
  window.history.replaceState({}, "", "#");
  mockedGetAppStatus.mockReset();
});

describe("FishMuse desktop shell", () => {
  it("resets_route_scroll_and_focuses_the_new_page_heading", async () => {
    localStorage.setItem("fishmuse.onboarding.complete", "true");
    window.history.replaceState({}, "", "#/library");
    mockedGetAppStatus.mockResolvedValue({
      version: "0.1.0",
      database: "ready",
      playback: { status: "disconnected", implementation: null },
      ai: { status: "not_configured", implementation: null },
    });

    const { container } = render(<App />);
    await screen.findByRole("heading", { name: "Library" });
    const viewport = container.querySelector<HTMLElement>(".page-scroll");
    expect(viewport).not.toBeNull();
    if (!viewport) throw new Error("route viewport missing");
    viewport.scrollTop = 180;

    window.location.hash = "#/now-playing";
    window.dispatchEvent(new HashChangeEvent("hashchange"));

    const heading = await screen.findByRole("heading", { name: "Now Playing" });
    await waitFor(() => {
      expect(viewport.scrollTop).toBe(0);
      expect(heading.tabIndex).toBe(-1);
      expect(document.activeElement).toBe(heading);
    });
  });

  it("maps the application DTO into provider-neutral service states", async () => {
    mockedGetAppStatus.mockResolvedValue({
      version: "0.1.0",
      database: "ready",
      playback: { status: "disconnected", implementation: null },
      ai: { status: "unavailable", implementation: null },
    });

    render(<App />);

    expect(screen.getByRole("heading", { name: "FishMuse" })).toBeTruthy();
    expect(screen.getByText("Local-first")).toBeTruthy();

    await waitFor(() => {
      expect(mockedGetAppStatus).toHaveBeenCalledOnce();
      expect(screen.getByText("Database: Ready")).toBeTruthy();
      expect(screen.getByText("Playback: Disconnected")).toBeTruthy();
      expect(screen.getByText("AI: Unavailable")).toBeTruthy();
    });
    expect(retryPlaybackService).not.toHaveBeenCalled();
  });

  it("keeps the local-first shell usable when application services are unavailable", async () => {
    mockedGetAppStatus.mockRejectedValue(new Error("Tauri is unavailable"));

    render(<App />);

    await waitFor(() => {
      expect(screen.getByText("Database: Unavailable")).toBeTruthy();
      expect(screen.getByText("Playback: Unavailable")).toBeTruthy();
      expect(screen.getByText("AI: Not Configured")).toBeTruthy();
    });
  });
});
