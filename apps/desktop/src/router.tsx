import { useEffect, useState } from "react";

import { LibraryPage } from "./features/library/LibraryPage";
import { OnboardingPage } from "./features/onboarding/OnboardingPage";
import { SettingsPage } from "./features/settings/SettingsPage";

type Route = "onboarding" | "library" | "chat" | "now-playing" | "settings";

function currentRoute(): Route {
  if (localStorage.getItem("fishmuse.onboarding.complete") !== "true") return "onboarding";
  const route = window.location.hash.replace(/^#\/?/, "");
  if (route === "settings" || route === "chat" || route === "now-playing") return route;
  return "library";
}

export function Router() {
  const [route, setRoute] = useState<Route>(currentRoute);
  useEffect(() => {
    const onHashChange = () => setRoute(currentRoute());
    window.addEventListener("hashchange", onHashChange);
    return () => window.removeEventListener("hashchange", onHashChange);
  }, []);

  if (route === "onboarding") return <OnboardingPage />;
  if (route === "settings") return <SettingsPage />;
  if (route === "chat") return <section className="empty-state"><h1>Ask FishMuse</h1><p>Streaming conversation arrives in the next interface step.</p></section>;
  if (route === "now-playing") return <section className="empty-state"><h1>Now Playing</h1><p>Choose a playable track from your Library.</p></section>;
  return <LibraryPage />;
}
