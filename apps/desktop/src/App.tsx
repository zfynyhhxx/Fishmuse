import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";

type ServiceState = "unavailable" | "not_configured";

type HealthStatus = {
  version: string;
  database: ServiceState;
  playback: ServiceState;
  ai: ServiceState;
};

const fallbackHealthStatus: HealthStatus = {
  version: "unknown",
  database: "unavailable",
  playback: "unavailable",
  ai: "not_configured",
};

function isServiceState(value: unknown): value is ServiceState {
  return value === "unavailable" || value === "not_configured";
}

function isHealthStatus(value: unknown): value is HealthStatus {
  if (!value || typeof value !== "object") {
    return false;
  }

  const status = value as Record<string, unknown>;
  return (
    typeof status.version === "string" &&
    isServiceState(status.database) &&
    isServiceState(status.playback) &&
    isServiceState(status.ai)
  );
}

function formatServiceState(state: ServiceState) {
  return state === "not_configured" ? "Not configured" : "Unavailable";
}

export default function App() {
  const [healthStatus, setHealthStatus] = useState<HealthStatus>(fallbackHealthStatus);

  useEffect(() => {
    let isMounted = true;

    void invoke<unknown>("healthcheck")
      .then((status) => {
        if (isMounted && isHealthStatus(status)) {
          setHealthStatus(status);
        }
      })
      .catch(() => {
        if (isMounted) {
          setHealthStatus(fallbackHealthStatus);
        }
      });

    return () => {
      isMounted = false;
    };
  }, []);

  const serviceStates = [
    ["Database", formatServiceState(healthStatus.database)],
    ["Playback", formatServiceState(healthStatus.playback)],
    ["AI", formatServiceState(healthStatus.ai)],
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
