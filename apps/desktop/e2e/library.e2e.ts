import { $, expect } from "@wdio/globals";

import { completeOnboarding, installDeterministicFakes } from "./harness";

describe("V0.1 Library playback", () => {
  it("plays a local result and updates the global MiniPlayer", async () => {
    await installDeterministicFakes();
    await completeOnboarding();
    await expect($("h1=Library")).toBeDisplayed();
    await expect($("strong=So What")).toBeDisplayed();

    await $('button[aria-label="Play So What"]').click();
    const player = await $('aside[aria-label="Mini player"]');
    await expect(player.$("strong=Current track")).toBeDisplayed();
    await expect(player.$("span=playing")).toBeDisplayed();
  });
});
