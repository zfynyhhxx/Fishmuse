import { $, expect } from "@wdio/globals";

import { SCAN_ID, emitEvent, installDeterministicFakes } from "./harness";

describe("V0.1 onboarding", () => {
  it("selects a fixture folder, scans locally, and searches the Library", async () => {
    await installDeterministicFakes();
    await expect($("h1=Your music stays yours")).toBeDisplayed();

    await $("button=Choose music folders").click();
    await expect($("a=Open Library")).toBeDisplayed();
    await emitEvent("fishmuse://scan-progress", {
      scan_id: SCAN_ID,
      discovered: 1,
      parsed: 1,
      unchanged: 0,
      failed: 0,
      status: "completed",
      error: null,
    });

    await expect($("p=1 of 1 tracks inspected · 0 need attention")).toBeDisplayed();
    await $("a=Open Library").click();
    const search = await $('input[placeholder="Title, artist, or release"]');
    await search.setValue("Kind of Blue");
    await expect($("strong=So What")).toBeDisplayed();
  });
});
