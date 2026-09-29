import { createContext, createElement, useContext, useEffect, useMemo, useState, type ReactNode } from "react";

import type { AppStatus, ScanProgress } from "../contracts";
import {
  getAppStatus,
  getPlaybackState,
  listenForPlaybackState,
  listenForScanProgress,
  listenForServiceState,
} from "../lib/ipc";
import { playbackStore } from "./playbackStore";

export const fallbackStatus: AppStatus = {
  version: "unknown",
  database: "unavailable",
  playback: { status: "unavailable", implementation: null },
  ai: { status: "not_configured", implementation: null },
};

type AppStore = {
  status: AppStatus;
  scanProgress: ScanProgress | null;
  refreshStatus: () => Promise<void>;
};

const AppStoreContext = createContext<AppStore | null>(null);

export function AppStoreProvider({ children }: { children: ReactNode }) {
  const [status, setStatus] = useState<AppStatus>(fallbackStatus);
  const [scanProgress, setScanProgress] = useState<ScanProgress | null>(null);

  const refreshStatus = async () => {
    try {
      setStatus(await getAppStatus());
    } catch {
      setStatus(fallbackStatus);
    }
  };

  useEffect(() => {
    let mounted = true;
    const cleanups: Array<() => void> = [];
    void getAppStatus()
      .then((next) => {
        if (mounted) setStatus(next);
      })
      .catch(() => {
        if (mounted) setStatus(fallbackStatus);
      });
    void getPlaybackState()
      .then((snapshot) => {
        if (mounted && snapshot) playbackStore.accept(snapshot);
      })
      .catch(() => undefined);
    void listenForScanProgress((progress) => {
      if (mounted) setScanProgress(progress);
    }).then((unlisten) => cleanups.push(unlisten)).catch(() => undefined);
    void listenForPlaybackState((snapshot) => {
      if (mounted) playbackStore.accept(snapshot);
    }).then((unlisten) => cleanups.push(unlisten)).catch(() => undefined);
    void listenForServiceState((service) => {
      if (mounted) setStatus((current) => ({ ...current, ...service }));
    }).then((unlisten) => cleanups.push(unlisten)).catch(() => undefined);
    return () => {
      mounted = false;
      cleanups.forEach((unlisten) => unlisten());
    };
  }, []);

  const value = useMemo(
    () => ({ status, scanProgress, refreshStatus }),
    [status, scanProgress],
  );
  return createElement(AppStoreContext.Provider, { value }, children);
}

export function useAppStore() {
  const value = useContext(AppStoreContext);
  if (!value) throw new Error("useAppStore must be used inside AppStoreProvider");
  return value;
}
