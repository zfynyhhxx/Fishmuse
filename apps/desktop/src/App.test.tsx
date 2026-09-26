import { render, screen } from "@testing-library/react";

import App from "./App";

describe("FishMuse desktop shell", () => {
  it("renders a usable local-first shell without AI or foobar", () => {
    render(<App />);

    expect(screen.getByRole("heading", { name: "FishMuse" })).toBeTruthy();
    expect(screen.getByText("Local-first")).toBeTruthy();
    expect(screen.getByText("AI: Not configured")).toBeTruthy();
    expect(screen.getByText("Playback: Unavailable")).toBeTruthy();
  });
});
