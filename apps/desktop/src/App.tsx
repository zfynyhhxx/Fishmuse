import { useEffect, useState } from "react";

import type { AppStatus } from "./contracts";
import { getAppStatus } from "./lib/ipc";

const fallbackStatus: AppStatus = {
  version: "unknown",
  database: "unavailable",
  playback: { status: "unavailable", implementation: null },
  ai: { status: "not_configured", implementation: null },
};

function formatServiceState(state: string) {
  return state
    .split("_")
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(" ");
}

export default function App() {
  const [status, setStatus] = useState<AppStatus>(fallbackStatus);

  useEffect(() => {
    let isMounted = true;
    void getAppStatus()
      .then((next) => {
        if (isMounted) setStatus(next);
      })
      .catch(() => {
        if (isMounted) setStatus(fallbackStatus);
      });
    return () => {
      isMounted = false;
    };
  }, []);

  const serviceStates = [
    ["Database", formatServiceState(status.database)],
    ["Playback", formatServiceState(status.playback.status)],
    ["AI", formatServiceState(status.ai.status)],
  ] as const;

  return (
    <main className="app-shell">
      <section aria-labelledby="product-name">
        <p className="eyebrow">Local-first</p>
        <h1 id="product-name">FishMuse</h1>
        <p>Your local music companion is ready for its next connection.</p>
      </section>

      <section aria-label="Service status">
        <h2>Service status</h2>
        <ul className="service-list">
          {serviceStates.map(([service, state]) => (
            <li key={service}>
              {service}: {state}
            </li>
          ))}
        </ul>
      </section>
    </main>
  );
}
