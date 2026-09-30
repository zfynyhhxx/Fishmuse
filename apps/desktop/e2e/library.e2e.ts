import { $, browser, expect } from "@wdio/globals";

import { completeOnboarding, installDeterministicFakes } from "./harness";

describe("V0.1 Library playback", () => {
  it("keeps Library chrome fixed while only the track viewport scrolls", async () => {
    await browser.setWindowSize(1000, 700);
    await installDeterministicFakes();
    await browser.execute(() => {
      const commands = window.__FISHMUSE_E2E_COMMANDS__;
      const seed = (commands?.search_library as Array<Record<string, unknown>>)[0];
      if (commands) {
        commands.search_library = Array.from({ length: 100 }, (_, index) => ({
          ...seed,
          id: `track-${index}`,
          title: `Track ${index}`,
        }));
      }
    });
    await completeOnboarding();
    await expect($("h1=Library")).toBeDisplayed();
    await expect($("strong=Track 0")).toBeDisplayed();

    const geometry = await browser.execute(() => {
      const page = document.querySelector<HTMLElement>(".page-scroll");
      const tracks = document.querySelector<HTMLElement>(".track-viewport");
      const player = document.querySelector<HTMLElement>('[aria-label="Mini player"]');
      if (!page || !tracks || !player) throw new Error("Library layout nodes missing");
      tracks.scrollTop = 1_000;
      tracks.dispatchEvent(new Event("scroll"));
      const playerRect = player.getBoundingClientRect();
      return {
        pageScrollTop: page.scrollTop,
        pageScrollHeight: page.scrollHeight,
        pageClientHeight: page.clientHeight,
        trackScrollTop: tracks.scrollTop,
        trackScrollable: tracks.scrollHeight > tracks.clientHeight,
        playerVisible: playerRect.top >= -1 && playerRect.bottom <= window.innerHeight + 1,
      };
    });

    expect(geometry.pageScrollTop).toBe(0);
    expect(geometry.pageScrollHeight).toBe(geometry.pageClientHeight);
    expect(geometry.trackScrollTop).toBeGreaterThan(0);
    expect(geometry.trackScrollable).toBe(true);
    expect(geometry.playerVisible).toBe(true);
  });

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
