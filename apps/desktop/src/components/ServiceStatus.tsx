import type { AppStatus } from "../contracts";

const label = (value: string) =>
  value.split("_").map((part) => part[0]?.toUpperCase() + part.slice(1)).join(" ");

export function ServiceStatus({ status }: { status: AppStatus }) {
  return (
    <section className="service-strip" aria-label="Service status">
      <span>Database: {label(status.database)}</span>
      <span>Playback: {label(status.playback.status)}</span>
      <span>AI: {label(status.ai.status)}</span>
    </section>
  );
}
