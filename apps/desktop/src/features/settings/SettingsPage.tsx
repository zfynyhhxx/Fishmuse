import { useEffect, useState } from "react";

import type { AISettings } from "../../contracts";
import {
  configureDeepSeekKey,
  deleteDeepSeekKey,
  getAISettings,
  launchPlaybackBackend,
} from "../../lib/ipc";
import { useAppStore } from "../../state/appStore";

const titleCase = (value: string) => value.charAt(0).toUpperCase() + value.slice(1);

export function SettingsPage() {
  const { status, refreshStatus } = useAppStore();
  const [settings, setSettings] = useState<AISettings | null>(null);
  const [apiKey, setApiKey] = useState("");
  const [notice, setNotice] = useState<string | null>(null);
  const [launchingPlayback, setLaunchingPlayback] = useState(false);

  const refresh = async () => setSettings(await getAISettings());
  useEffect(() => {
    let mounted = true;
    void getAISettings()
      .then((next) => {
        if (mounted) setSettings(next);
      })
      .catch(() => {
        if (mounted) setNotice("AI settings are currently unavailable.");
      });
    return () => { mounted = false; };
  }, []);

  const save = async () => {
    if (!apiKey.trim()) return;
    try {
      await configureDeepSeekKey(apiKey);
      setApiKey("");
      setNotice("API key saved securely.");
      await Promise.all([refresh(), refreshStatus()]);
    } catch {
      setNotice("The API key could not be saved.");
    }
  };

  const remove = async () => {
    await deleteDeepSeekKey();
    setApiKey("");
    setNotice("API key deleted.");
    await Promise.all([refresh(), refreshStatus()]);
  };

  const startPlayback = async () => {
    setLaunchingPlayback(true);
    try {
      await launchPlaybackBackend();
      setNotice("foobar2000 started. Waiting for the FishMuse component to connect.");
      await refreshStatus();
    } catch {
      setNotice("foobar2000 could not be started. Confirm it is installed and try again.");
    } finally {
      setLaunchingPlayback(false);
    }
  };

  const budgetWarning = settings?.budget &&
    settings.budget.spent_microunits >= settings.budget.warning_at_microunits;
  const provider = settings?.service.implementation?.display_name ??
    (settings ? titleCase(settings.provider) : "DeepSeek");
  const playback = status.playback.implementation?.display_name ?? "Not selected";

  return (
    <section className="page-stack" aria-labelledby="settings-title">
      <div>
        <p className="eyebrow">Preferences</p>
        <h1 id="settings-title">Settings</h1>
      </div>
      <article className="panel">
        <h2>AI service</h2>
        <p>Provider: {provider}</p>
        <p>Model: {settings?.model ?? "deepseek-flash"}</p>
        <p>Status: {settings?.configured ? "Configured" : "Not configured"}</p>
        {budgetWarning ? <p className="warning-banner">The live test budget has reached the warning level.</p> : null}
        {settings?.budget ? (
          <p>Estimated spend: ¥{(settings.budget.spent_microunits / 1_000_000).toFixed(2)} / ¥{(settings.budget.hard_stop_at_microunits / 1_000_000).toFixed(2)} live-test stop</p>
        ) : null}
        <label htmlFor="deepseek-key">DeepSeek API key</label>
        <input
          id="deepseek-key"
          type="password"
          autoComplete="off"
          value={apiKey}
          onChange={(event) => setApiKey(event.target.value)}
          placeholder="Stored in Windows Credential Manager"
        />
        <div className="button-row">
          <button type="button" onClick={() => void save()}>Save API key</button>
          <button className="secondary" type="button" onClick={() => void remove()}>Delete API key</button>
        </div>
      </article>
      <article className="panel">
        <h2>Playback service</h2>
        <p>Playback backend: {playback}</p>
        <p>Status: {status.playback.status}</p>
        {status.playback.status !== "ready" ? (
          <button type="button" disabled={launchingPlayback} onClick={() => void startPlayback()}>
            {launchingPlayback ? "Starting foobar2000…" : "Start foobar2000"}
          </button>
        ) : null}
      </article>
      {notice ? <p role="status">{notice}</p> : null}
    </section>
  );
}
