import { $, expect } from "@wdio/globals";

import { completeOnboarding, emitAI, installDeterministicFakes, navigate, waitForCommand } from "./harness";

describe("V0.1 fake AI chat", () => {
  it("shows a library tool activity and a streamed answer", async () => {
    await installDeterministicFakes();
    await completeOnboarding();
    await navigate("#/chat");
    await expect($("h1=Ask FishMuse")).toBeDisplayed();

    await $('textarea[aria-label="Message FishMuse"]').setValue("Find Miles Davis");
    await $("button=Send").click();
    await waitForCommand("start_ai_turn");
    await emitAI(1, { event_type: "turn_started" });
    await emitAI(2, { event_type: "tool_started", payload: { id: "tool-1", name: "search_library" } });
    await emitAI(3, { event_type: "tool_finished", payload: { id: "tool-1", name: "search_library", result: { count: 1 } } });
    await emitAI(4, { event_type: "text_delta", payload: { delta: "I found " } });
    await emitAI(5, { event_type: "text_delta", payload: { delta: "So What." } });
    await emitAI(6, { event_type: "turn_completed" });

    await expect($("strong=Searched your library")).toBeDisplayed();
    await expect($("p=I found So What.")).toBeDisplayed();
  });
});
