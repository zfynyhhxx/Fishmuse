import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import App from "./App";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

const mockedInvoke = vi.mocked(invoke);

afterEach(cleanup);

beforeEach(() => {
  mockedInvoke.mockReset();
});

describe("FishMuse desktop shell", () => {
  it("maps the Rust healthcheck DTO into service states", async () => {
    mockedInvoke.mockResolvedValue({
      version: "0.1.0",
      database: "not_configured",
      playback: "unavailable",
      ai: "unavailable",
    });

    render(<App />);

    expect(screen.getByRole("heading", { name: "FishMuse" })).toBeTruthy();
    expect(screen.getByText("Local-first")).toBeTruthy();

    await waitFor(() => {
      expect(mockedInvoke).toHaveBeenCalledWith("healthcheck");
      expect(screen.getByText("Database: Not configured")).toBeTruthy();
      expect(screen.getByText("Playback: Unavailable")).toBeTruthy();
      expect(screen.getByText("AI: Unavailable")).toBeTruthy();
    });
  });

  it("keeps the local-first shell usable when healthcheck is unavailable", async () => {
    mockedInvoke.mockRejectedValue(new Error("Tauri is unavailable"));

    render(<App />);

    await waitFor(() => {
      expect(screen.getByText("Database: Unavailable")).toBeTruthy();
      expect(screen.getByText("Playback: Unavailable")).toBeTruthy();
      expect(screen.getByText("AI: Not configured")).toBeTruthy();
    });
  });
});
