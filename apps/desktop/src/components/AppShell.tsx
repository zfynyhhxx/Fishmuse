import type { ReactNode } from "react";

import { useAppStore } from "../state/appStore";
import { MiniPlayer } from "./MiniPlayer";
import { ServiceStatus } from "./ServiceStatus";

const links = [
  ["Library", "#/library"],
  ["Ask FishMuse", "#/chat"],
  ["Now Playing", "#/now-playing"],
  ["Settings", "#/settings"],
] as const;

export function AppShell({ children }: { children: ReactNode }) {
  const { status } = useAppStore();
  return (
    <div className="shell-grid">
      <header className="brand-block">
        <p className="eyebrow">Local-first</p>
        <h1 className="brand-heading"><a className="brand" href="#/library">FishMuse</a></h1>
      </header>
      <nav className="primary-nav" aria-label="Primary navigation">
        {links.map(([name, href]) => <a href={href} key={href}>{name}</a>)}
      </nav>
      <ServiceStatus status={status} />
      <main className="page-content">{children}</main>
      <MiniPlayer />
    </div>
  );
}
