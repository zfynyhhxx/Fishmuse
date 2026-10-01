import { $, $$, browser, expect } from "@wdio/globals";

import {
  SECOND_TRACK_ID,
  TRACK_ID,
  commandCalls,
  completeOnboarding,
  emitAI,
  emitEvent,
  installDeterministicFakes,
  navigate,
  waitForCommand,
  waitForCommandCount,
} from "./harness";

const artwork = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";

const view = (overrides: Record<string, unknown> = {}) => ({
  revision: 10,
  status: "playing",
  position_ms: 2_000,
  duration_ms: 545_000,
  volume: 0.7,
  muted: false,
  track: {
    id: TRACK_ID,
    title: "So What",
    artist_names: ["Miles Davis"],
    release_title: "Kind of Blue",
    artwork_available: true,
  },
  queue: {
    track_ids: [TRACK_ID, SECOND_TRACK_ID],
    current_index: 0,
    can_previous: true,
    can_next: true,
  },
  external: false,
  ...overrides,
});

describe("V0.1 fake playback synchronization", () => {
  it("projects an AI playback tool into Now Playing", async () => {
    await installDeterministicFakes();
    await completeOnboarding();
    await navigate("#/chat");

    await $('textarea[aria-label="Message FishMuse"]').setValue("Play So What");
    await $("button=Send").click();
    await waitForCommand("start_ai_turn");
    await emitAI(1, { event_type: "turn_started" });
    await emitAI(2, { event_type: "tool_started", payload: { id: "tool-1", name: "play_track" } });
    await emitEvent("fishmuse://playback-state", {
      revision: 1,
      status: "playing",
      position_ms: 2_000,
      duration_ms: 545_000,
      volume: 1,
      muted: false,
      track: {
        id: TRACK_ID,
        title: "So What",
        artist_names: ["Miles Davis"],
        release_title: "Kind of Blue",
        artwork_available: false,
      },
      queue: { track_ids: [TRACK_ID], current_index: 0, can_previous: true, can_next: false },
      external: false,
    });
    await emitAI(3, { event_type: "tool_finished", payload: { id: "tool-1", name: "play_track", result: { status: "playing" } } });
    await emitAI(4, { event_type: "text_delta", payload: { delta: "Playing So What." } });
    await emitAI(5, { event_type: "turn_completed" });

    await navigate("#/now-playing");
    await expect($("h1=Now Playing")).toBeDisplayed();
    await expect($("h2=So What")).toBeDisplayed();
    await expect($("p=Miles Davis")).toBeDisplayed();
    await expect($("p=Kind of Blue")).toBeDisplayed();
    await expect($("span=playing")).toBeDisplayed();
  });

  it("shows generic managed startup without exposing the backend implementation", async () => {
    await installDeterministicFakes();
    await completeOnboarding();

    await emitEvent("fishmuse://service-state", {
      playback: { status: "starting", implementation: null },
      ai: { status: "ready", implementation: null },
    });
    await expect($("span=Playback: Starting")).toBeDisplayed();
    expect((await $("body").getText()).toLowerCase()).not.toContain("foobar");

    await emitEvent("fishmuse://service-state", {
      playback: { status: "ready", implementation: null },
      ai: { status: "ready", implementation: null },
    });
    await expect($("span=Playback: Ready")).toBeDisplayed();
  });

  it("renders safe artwork and metadata and sends every playback control", async () => {
    await installDeterministicFakes();
    await browser.execute((artworkData) => {
      if (window.__FISHMUSE_E2E_COMMANDS__) {
        window.__FISHMUSE_E2E_COMMANDS__.get_track_artwork = { data_url: artworkData };
      }
    }, artwork);
    await completeOnboarding();
    await emitEvent("fishmuse://playback-state", view());
    await navigate("#/now-playing");

    await expect($("h2=So What")).toBeDisplayed();
    await expect($("p=Miles Davis")).toBeDisplayed();
    await expect($("p=Kind of Blue")).toBeDisplayed();
    await expect($('img[alt="Artwork for So What"]')).toHaveAttribute("src", artwork);
    await expect($("span=playing")).toBeDisplayed();
    await expect($("p=0:02 / 9:05")).toBeDisplayed();
    const bodyText = await $("body").getText();
    expect(bodyText).not.toMatch(/[A-Z]:\\/);
    expect(bodyText.toLowerCase()).not.toContain("foobar");

    await $("button=Pause").click();
    await waitForCommandCount("execute_playback", 1);
    await emitEvent("fishmuse://playback-state", view({ revision: 11, status: "paused" }));
    await $("button=Resume").click();
    await waitForCommandCount("execute_playback", 2);

    await $('input[aria-label="Seek position"]').setValue(31_000);
    await $("button=Seek").click();
    await waitForCommandCount("execute_playback", 3);
    await $('input[aria-label="Volume"]').setValue(35);
    await $("button=Set volume").click();
    await waitForCommandCount("execute_playback", 4);
    await $("button=Mute").click();
    await waitForCommandCount("execute_playback", 5);
    await $("button=Stop").click();
    await waitForCommandCount("execute_playback", 6);

    await browser.execute((result) => {
      if (window.__FISHMUSE_E2E_COMMANDS__) window.__FISHMUSE_E2E_COMMANDS__.execute_queue_command = result;
    }, { track_ids: [TRACK_ID, SECOND_TRACK_ID], current_index: 1, can_previous: true, can_next: false });
    await $('button[aria-label="Play queued track 2"]').click();
    await waitForCommandCount("execute_queue_command", 1);
    await expect($("span=Queued track 2 · Playing")).toBeDisplayed();

    await browser.execute((result) => {
      if (window.__FISHMUSE_E2E_COMMANDS__) window.__FISHMUSE_E2E_COMMANDS__.execute_queue_command = result;
    }, { track_ids: [TRACK_ID], current_index: 0, can_previous: true, can_next: false });
    await $('button[aria-label="Remove queued track 2"]').click();
    await waitForCommandCount("execute_queue_command", 2);
    expect(await $$('button[aria-label^="Remove queued track"]')).toHaveLength(1);
    await emitEvent("fishmuse://playback-state", view({ revision: 13 }));

    await browser.execute((result) => {
      if (window.__FISHMUSE_E2E_COMMANDS__) window.__FISHMUSE_E2E_COMMANDS__.execute_queue_command = result;
    }, { track_ids: [TRACK_ID, SECOND_TRACK_ID], current_index: 1, can_previous: true, can_next: false });
    await $("button=Next track").click();
    await waitForCommandCount("execute_queue_command", 3);
    await expect($("span=Queued track 2 · Playing")).toBeDisplayed();

    await browser.execute((result) => {
      if (window.__FISHMUSE_E2E_COMMANDS__) window.__FISHMUSE_E2E_COMMANDS__.execute_queue_command = result;
    }, { track_ids: [TRACK_ID, SECOND_TRACK_ID], current_index: 0, can_previous: true, can_next: true });
    await $("button=Previous track").click();
    await waitForCommandCount("execute_queue_command", 4);
    await expect($("span=So What · Playing")).toBeDisplayed();

    await browser.execute((result) => {
      if (window.__FISHMUSE_E2E_COMMANDS__) window.__FISHMUSE_E2E_COMMANDS__.execute_queue_command = result;
    }, { track_ids: [], current_index: null, can_previous: false, can_next: false });
    await $("button=Clear queue").click();
    await waitForCommandCount("execute_queue_command", 5);
    await expect($("p=Queue is empty.")).toBeDisplayed();

    const playbackKinds = (await commandCalls("execute_playback"))
      .map((call) => (call as { command: { kind: string } }).command.kind);
    expect(playbackKinds).toEqual(["pause", "resume", "seek", "set_volume", "set_volume", "stop"]);
    const queueKinds = (await commandCalls("execute_queue_command"))
      .map((call) => (call as { command: { kind: string } }).command.kind);
    expect(queueKinds).toEqual(["play_at", "remove", "next", "previous", "clear"]);
  });

  it("shows external playback and generic control failures safely", async () => {
    await installDeterministicFakes();
    await completeOnboarding();
    await emitEvent("fishmuse://playback-state", view({
      revision: 30,
      track: null,
      queue: { track_ids: [], current_index: null, can_previous: false, can_next: false },
      external: true,
    }));
    await navigate("#/now-playing");
    await expect($("h2=External playback")).toBeDisplayed();

    await browser.execute(() => {
      delete window.__FISHMUSE_E2E_COMMANDS__?.execute_playback;
    });
    await $("button=Pause").click();
    await expect($('[role="alert"]')).toBeDisplayed();
    const errorText = await $('[role="alert"]').getText();
    expect(errorText).toContain("Playback control is unavailable");
    expect(errorText.toLowerCase()).not.toContain("foobar");
    await expect($("a=Advanced diagnostics")).toHaveAttribute("href", "#/settings");
    expect(await $$("button=Pause")).toHaveLength(1);
  });
});
