import { useEffect, useState } from "react";

import { AppShell } from "./components/AppShell";
import { LibraryPage } from "./features/library/LibraryPage";
import { ChatPage } from "./features/chat/ChatPage";
import { NowPlayingPage } from "./features/now-playing/NowPlayingPage";
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
  let page;
  if (route === "onboarding") page = <OnboardingPage />;
  else if (route === "settings") page = <SettingsPage />;
  else if (route === "chat") page = <ChatPage />;
  else if (route === "now-playing") page = <NowPlayingPage />;
  else page = <LibraryPage />;
  return <AppShell routeKey={route}>{page}</AppShell>;
}
