import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import App from "../../App";
import {
  chooseLibraryFolders,
  getAppStatus,
  listenForScanProgress,
  startLibraryScan,
} from "../../lib/ipc";

vi.mock("../../lib/ipc", () => ({
  chooseLibraryFolders: vi.fn(),
  getAppStatus: vi.fn(),
  getPlaybackState: vi.fn(async () => ({ revision: 0, status: "unavailable", track_id: null, position_ms: 0, duration_ms: null })),
  listenForPlaybackState: vi.fn(async () => vi.fn()),
  listenForScanProgress: vi.fn(),
  listenForServiceState: vi.fn(async () => vi.fn()),
  searchLibrary: vi.fn(async () => []),
  startLibraryScan: vi.fn(),
}));

const status = {
  version: "0.1.0",
  database: "ready" as const,
  playback: {
    status: "disconnected" as const,
    implementation: null,
  },
  ai: {
    status: "not_configured" as const,
    implementation: { id: "deepseek", display_name: "DeepSeek" },
  },
};

afterEach(cleanup);

beforeEach(() => {
  localStorage.clear();
  window.history.replaceState({}, "", "#");
  vi.mocked(getAppStatus).mockResolvedValue(status);
  vi.mocked(chooseLibraryFolders).mockResolvedValue(["C:\\Music"]);
  vi.mocked(startLibraryScan).mockResolvedValue({ scan_id: "01999999-9999-7999-8999-999999999991" });
});

describe("first-run onboarding", () => {
  it("explains local-first setup and starts a scan for the chosen folders", async () => {
    let publishProgress: ((progress: {
      scan_id: string;
      discovered: number;
      parsed: number;
      unchanged: number;
      failed: number;
      status: "running";
      error: null;
    }) => void) | undefined;
    vi.mocked(listenForScanProgress).mockImplementation(async (listener) => {
      publishProgress = listener;
      return () => undefined;
    });

    render(<App />);

    expect(screen.getByRole("heading", { name: "Your music stays yours" })).toBeTruthy();
    expect(screen.getByText(/FishMuse scans your folders locally/i)).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Choose music folders" }));

    await waitFor(() => {
      expect(chooseLibraryFolders).toHaveBeenCalledOnce();
      expect(startLibraryScan).toHaveBeenCalledWith(["C:\\Music"]);
    });

    act(() => publishProgress?.({
      scan_id: "01999999-9999-7999-8999-999999999991",
      discovered: 24,
      parsed: 10,
      unchanged: 4,
      failed: 1,
      status: "running",
      error: null,
    }));

    await waitFor(() => {
      expect(screen.getByText(/10 of 24 tracks inspected/i)).toBeTruthy();
      expect(screen.getByRole("link", { name: "Open Library" })).toBeTruthy();
    });
  });
});
