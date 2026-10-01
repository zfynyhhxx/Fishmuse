import { $, $$, browser, expect } from "@wdio/globals";
import { writeFile } from "node:fs/promises";

const fixtureRoot = process.env.FISHMUSE_LIVE_FIXTURE_ROOT;
const observationPath = process.env.FISHMUSE_LIVE_OBSERVATIONS;
const screenshotPath = process.env.FISHMUSE_LIVE_SCREENSHOT;
const monitorCompletePath = process.env.FISHMUSE_LIVE_MONITOR_COMPLETE;

if (!fixtureRoot || !observationPath || !screenshotPath || !monitorCompletePath) {
  throw new Error("The guarded live script must provide fixture, observation, screenshot, and monitor paths.");
}

const currentQueueIndex = () => browser.execute(() => {
  const rows = [...document.querySelectorAll(".queue-list > li")];
  return rows.findIndex((row) => row.classList.contains("queue-current"));
});

const setRangeValue = (label: string, value: number) => browser.execute((rangeLabel, nextValue) => {
  const input = document.querySelector<HTMLInputElement>(`input[aria-label="${rangeLabel}"]`);
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
  if (!input || !setter) throw new Error(`range input is unavailable: ${rangeLabel}`);
  setter.call(input, String(nextValue));
  input.dispatchEvent(new Event("input", { bubbles: true }));
  input.dispatchEvent(new Event("change", { bubbles: true }));
}, label, value);

const playbackSnapshot = () => browser.execute(async () => {
  const invoke = window.__TAURI__?.core?.invoke;
  if (!invoke) throw new Error("Tauri core.invoke is unavailable in the live harness");
  return invoke<{
    status: string;
    position_ms: number;
    volume: number;
    muted: boolean;
  }>("get_playback_state");
});

const assertNoControlError = async (stage: string) => {
  const alert = await $('[role="alert"]');
  if (await alert.isExisting()) {
    throw new Error(`${stage} left a playback error: ${await alert.getText()}`);
  }
};

describe("FishMuse real Windows playback", () => {
  it("starts hidden playback and drives every V0.1 control through FishMuse", async () => {
    await browser.execute(() => {
      localStorage.clear();
      localStorage.setItem("fishmuse.onboarding.complete", "true");
      window.location.hash = "#/library";
    });
    await expect($("h1=Library")).toBeDisplayed();
    await browser.execute(async () => {
      const internals = (window as typeof window & {
        __TAURI_INTERNALS__?: { metadata?: { currentWindow?: { label?: string } } };
      }).__TAURI_INTERNALS__;
      const label = internals?.metadata?.currentWindow?.label;
      const invoke = window.__TAURI__?.core?.invoke;
      if (!label || !invoke) throw new Error("The FishMuse native window focus boundary is unavailable.");
      await invoke("plugin:window|set_focus", { label });
    });

    await browser.execute(async (root) => {
      const invoke = window.__TAURI__?.core?.invoke;
      if (!invoke) throw new Error("Tauri core.invoke is unavailable in the live harness");
      await invoke("start_library_scan", { roots: [root] });
    }, fixtureRoot);
    await browser.waitUntil(async () => (await $$('button[aria-label^="Play "]')).length >= 2, {
      timeout: 30_000,
      timeoutMsg: "the isolated fixture scan did not produce two playable tracks",
    });

    const playButtons = await $$('button[aria-label^="Play "]');
    await playButtons[0].click();
    try {
      await browser.waitUntil(async () => (await $('aside[aria-label="Mini player"] strong').getText()) !== "Nothing playing", {
        timeout: 15_000,
        timeoutMsg: "FishMuse did not reach a playing track after managed startup",
      });
    } catch (error) {
      const diagnostic = await browser.execute(() => ({
        service: document.querySelector('[aria-label="Service status"]')?.textContent ?? null,
        alert: document.querySelector('[role="alert"]')?.textContent ?? null,
        miniPlayer: document.querySelector('[aria-label="Mini player"]')?.textContent ?? null,
      }));
      const commandError = await browser.execute(async () => {
        const invoke = window.__TAURI__?.core?.invoke;
        if (!invoke) return "Tauri core.invoke unavailable";
        const tracks = await invoke<Array<{ id: string }>>("search_library", {
          query: { text: "", artist: null, release: null, limit: 100, offset: 0 },
        });
        const now = Date.now().toString(16).padStart(12, "0");
        const random = crypto.randomUUID().replaceAll("-", "");
        const operationId = `${now.slice(0, 8)}-${now.slice(8)}-7${random.slice(13, 16)}-a${random.slice(17, 20)}-${random.slice(20, 32)}`;
        try {
          await invoke("execute_queue_command", {
            command: {
              kind: "play_now",
              track_id: tracks[0]?.id,
              context: tracks.map((track) => track.id),
              operation_id: operationId,
            },
          });
          return null;
        } catch (invokeError) {
          return invokeError;
        }
      });
      throw new Error(`${String(error)}; UI diagnostic: ${JSON.stringify(diagnostic)}; command error: ${JSON.stringify(commandError)}`);
    }
    await expect($('aside[aria-label="Mini player"] button[aria-label="Pause"]')).toBeDisplayed();

    await $("a=Now Playing").click();
    await expect($("h1=Now Playing")).toBeFocused();
    await expect($("span=playing")).toBeDisplayed();
    await browser.waitUntil(async () => (await $$(".queue-list > li")).length === 2);
    expect(await currentQueueIndex()).toBe(0);
    await browser.saveScreenshot(screenshotPath);

    await $("button=Pause").click();
    await expect($("span=paused")).toBeDisplayed();
    await assertNoControlError("pause");
    await $("button=Resume").click();
    await expect($("span=playing")).toBeDisplayed();
    await assertNoControlError("resume");

    const seek = await $('input[aria-label="Seek position"]');
    const duration = Number(await seek.getAttribute("max"));
    if (duration < 30_000) throw new Error("The live audio fixture must be at least 30 seconds long.");
    await setRangeValue("Seek position", 2_000);
    await expect(seek).toHaveValue("2000");
    await $("button=Seek").click();
    await browser.waitUntil(async () => {
      const snapshot = await playbackSnapshot();
      return snapshot.status === "playing" && snapshot.position_ms >= 1_500 && snapshot.position_ms < 10_000;
    });
    await assertNoControlError("seek");

    const volume = await $('input[aria-label="Volume"]');
    await setRangeValue("Volume", 35);
    await expect(volume).toHaveValue("35");
    await $("button=Set volume").click();
    try {
      await browser.waitUntil(async () => Math.abs((await playbackSnapshot()).volume - 0.35) <= 0.01);
    } catch (error) {
      const diagnostic = await browser.execute(async () => ({
        slider: (document.querySelector('input[aria-label="Volume"]') as HTMLInputElement | null)?.value ?? null,
        alert: document.querySelector('[role="alert"]')?.textContent ?? null,
        snapshot: await window.__TAURI__?.core?.invoke?.("get_playback_state"),
      }));
      throw new Error(`${String(error)}; volume diagnostic: ${JSON.stringify(diagnostic)}`);
    }
    await assertNoControlError("volume");
    await $("button=Mute").click();
    await expect($("button=Unmute")).toBeDisplayed();
    await assertNoControlError("mute");
    await $("button=Unmute").click();
    await expect($("button=Mute")).toBeDisplayed();
    await assertNoControlError("unmute");

    await $("button=Next track").click();
    await browser.waitUntil(async () => (await currentQueueIndex()) === 1);
    await assertNoControlError("next");
    await $("button=Previous track").click();
    await browser.waitUntil(async () => (await currentQueueIndex()) === 0);
    await assertNoControlError("previous");

    await $("button=Stop").click();
    await expect($("span=stopped")).toBeDisplayed();
    await assertNoControlError("stop");

    await writeFile(observationPath, JSON.stringify({
      playback_reached_playing: true,
      pause_resume: true,
      seek: true,
      volume: true,
      mute_unmute: true,
      next_previous: true,
      stop: true,
      ui_only_playback_commands: true,
    }), { encoding: "utf8" });
    await writeFile(monitorCompletePath, "complete", { encoding: "ascii" });
  });
});
