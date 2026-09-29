import { AppShell } from "./components/AppShell";
import { Router } from "./router";
import { AppStoreProvider } from "./state/appStore";

export default function App() {
  return (
    <AppStoreProvider>
      <AppShell><Router /></AppShell>
    </AppStoreProvider>
  );
}
