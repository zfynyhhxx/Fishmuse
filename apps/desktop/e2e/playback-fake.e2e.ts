import { $, expect } from "@wdio/globals";

import { TRACK_ID, completeOnboarding, emitAI, emitEvent, installDeterministicFakes, navigate, waitForCommand } from "./harness";

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
      track_id: TRACK_ID,
      position_ms: 2_000,
      duration_ms: 545_000,
    });
    await emitAI(3, { event_type: "tool_finished", payload: { id: "tool-1", name: "play_track", result: { status: "playing" } } });
    await emitAI(4, { event_type: "text_delta", payload: { delta: "Playing So What." } });
    await emitAI(5, { event_type: "turn_completed" });

    await navigate("#/now-playing");
    await expect($("h1=Now Playing")).toBeDisplayed();
    await expect($(`code=${TRACK_ID}`)).toBeDisplayed();
    await expect($("span=playing")).toBeDisplayed();
  });
});
