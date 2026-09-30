import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import App from "../../App";
import {
  configureDeepSeekKey,
  getAISettings,
  getAppStatus,
  retryPlaybackService,
} from "../../lib/ipc";

vi.mock("../../lib/ipc", () => ({
  configureDeepSeekKey: vi.fn(),
  deleteDeepSeekKey: vi.fn(),
  getAISettings: vi.fn(),
  getAppStatus: vi.fn(),
  retryPlaybackService: vi.fn(),
  getPlaybackState: vi.fn(async () => ({ revision: 0, status: "unavailable", track_id: null, position_ms: 0, duration_ms: null })),
  listenForPlaybackState: vi.fn(async () => vi.fn()),
  listenForScanProgress: vi.fn(async () => vi.fn()),
  listenForServiceState: vi.fn(async () => vi.fn()),
  searchLibrary: vi.fn(async () => []),
}));

afterEach(cleanup);

beforeEach(() => {
  localStorage.setItem("fishmuse.onboarding.complete", "true");
  window.history.replaceState({}, "", "#/settings");
  vi.mocked(getAppStatus).mockResolvedValue({
    version: "0.1.0",
    database: "ready",
    playback: {
      status: "disconnected",
      implementation: { id: "managed", display_name: "Managed playback service" },
    },
    ai: {
      status: "ready",
      implementation: { id: "deepseek", display_name: "DeepSeek" },
    },
  });
  vi.mocked(getAISettings).mockResolvedValue({
    configured: true,
    provider: "deepseek",
    model: "deepseek-flash",
    service: {
      status: "ready",
      implementation: { id: "deepseek", display_name: "DeepSeek" },
    },
    budget: {
      spent_microunits: 10_000_000,
      warning_at_microunits: 10_000_000,
      hard_stop_at_microunits: 20_000_000,
    },
  });
  vi.mocked(configureDeepSeekKey).mockResolvedValue();
  vi.mocked(retryPlaybackService).mockResolvedValue();
});

describe("settings", () => {
  it("keeps secrets masked, clears them after saving, and describes replaceable services", async () => {
    render(<App />);

    const keyInput = await screen.findByLabelText("DeepSeek API key");
    expect(keyInput.getAttribute("type")).toBe("password");
    expect(screen.getByText("Provider: DeepSeek")).toBeTruthy();
    expect(screen.getByText("Model: deepseek-flash")).toBeTruthy();
    expect(screen.getByText("Playback implementation: Managed playback service")).toBeTruthy();
    expect(screen.getByText(/live test budget has reached the warning level/i)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Retry playback service" })).toBeTruthy();

    fireEvent.change(keyInput, { target: { value: "sk-super-secret" } });
    fireEvent.click(screen.getByRole("button", { name: "Save API key" }));

    await waitFor(() => {
      expect(configureDeepSeekKey).toHaveBeenCalledWith("sk-super-secret");
      expect((keyInput as HTMLInputElement).value).toBe("");
    });
    expect(document.body.textContent).not.toContain("sk-super-secret");
  });

  it("keeps backend retry inside advanced diagnostics", async () => {
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Retry playback service" }));

    await waitFor(() => expect(retryPlaybackService).toHaveBeenCalledOnce());
    expect(screen.getByRole("status").textContent).toMatch(
      /playback service retry requested/i,
    );
  });
});
