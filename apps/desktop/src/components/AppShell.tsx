import { useLayoutEffect, useRef, type ReactNode } from "react";

import { useAppStore } from "../state/appStore";
import { MiniPlayer } from "./MiniPlayer";
import { ServiceStatus } from "./ServiceStatus";

const links = [
  ["Library", "#/library"],
  ["Ask FishMuse", "#/chat"],
  ["Now Playing", "#/now-playing"],
  ["Settings", "#/settings"],
] as const;

export function AppShell({ children, routeKey }: { children: ReactNode; routeKey: string }) {
  const { status } = useAppStore();
  const viewport = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    const element = viewport.current;
    if (!element) return;
    const heading = element.querySelector<HTMLHeadingElement>("h1");
    if (heading) {
      heading.tabIndex = -1;
      heading.focus({ preventScroll: true });
    }
    element.scrollTop = 0;
    element.scrollLeft = 0;
  }, [routeKey]);

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
      <main className="page-content">
        <div className="page-scroll" ref={viewport}>{children}</div>
      </main>
      <MiniPlayer />
    </div>
  );
}
