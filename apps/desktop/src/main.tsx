import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import "./styles/global.css";

async function enableE2EHarness() {
  if (import.meta.env.MODE === "e2e") {
    await import("@wdio/tauri-plugin");
  }
}

async function main() {
  await enableE2EHarness();
  const { default: App } = await import("./App");
  const rootElement = document.getElementById("root");

  if (!rootElement) {
    throw new Error("FishMuse root element is missing");
  }

  createRoot(rootElement).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
}

void main();
