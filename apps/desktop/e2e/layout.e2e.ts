import { $, browser, expect } from "@wdio/globals";

import { completeOnboarding, installDeterministicFakes } from "./harness";

const viewports = [
  { width: 1000, height: 700 },
  { width: 1440, height: 900 },
  { width: 720, height: 520 },
] as const;

describe("V0.1 fixed desktop shell", () => {
  beforeEach(async () => {
    await installDeterministicFakes();
    await completeOnboarding();
    await expect($("h1=Library")).toBeDisplayed();
  });

  for (const viewport of viewports) {
    it(`keeps document chrome fixed at ${viewport.width}x${viewport.height}`, async () => {
      await browser.setWindowSize(viewport.width, viewport.height);

      const geometry = await browser.execute(() => {
        const player = document.querySelector<HTMLElement>('[aria-label="Mini player"]');
        const playerRect = player?.getBoundingClientRect();
        return {
          documentHeight: document.documentElement.scrollHeight,
          documentClientHeight: document.documentElement.clientHeight,
          bodyHeight: document.body.scrollHeight,
          bodyClientHeight: document.body.clientHeight,
          documentWidth: document.documentElement.scrollWidth,
          documentClientWidth: document.documentElement.clientWidth,
          playerVisible: Boolean(playerRect && playerRect.top >= -1 && playerRect.bottom <= window.innerHeight + 1),
        };
      });

      expect(geometry.documentHeight).toBe(geometry.documentClientHeight);
      expect(geometry.bodyHeight).toBe(geometry.bodyClientHeight);
      expect(geometry.documentWidth).toBe(geometry.documentClientWidth);
      expect(geometry.playerVisible).toBe(true);
    });
  }

  it("keeps the Library track viewport usable at the minimum window size", async () => {
    await browser.setWindowSize(720, 520);

    const geometry = await browser.execute(() => {
      const tracks = document.querySelector<HTMLElement>(".track-viewport");
      const firstPlay = document.querySelector<HTMLElement>('.track-row button[aria-label^="Play "]');
      const content = document.querySelector<HTMLElement>(".page-content");
      if (!tracks || !firstPlay || !content) throw new Error("Library layout nodes missing");
      const playRect = firstPlay.getBoundingClientRect();
      const contentRect = content.getBoundingClientRect();
      return {
        trackViewportHeight: tracks.clientHeight,
        firstPlayVisible: playRect.top >= contentRect.top && playRect.bottom <= contentRect.bottom,
      };
    });

    expect(geometry.trackViewportHeight).toBeGreaterThanOrEqual(58);
    expect(geometry.firstPlayVisible).toBe(true);
  });

  for (const viewport of [{ width: 1000, height: 700 }, { width: 720, height: 520 }]) {
    it(`keeps Now Playing free of horizontal overflow at ${viewport.width}x${viewport.height}`, async () => {
      await browser.setWindowSize(viewport.width, viewport.height);
      await $("a=Now Playing").click();
      await expect($("h1=Now Playing")).toBeDisplayed();

      const geometry = await browser.execute(() => {
        const page = document.querySelector<HTMLElement>(".page-scroll");
        const card = document.querySelector<HTMLElement>(".now-playing-card");
        if (!page || !card) throw new Error("Now Playing layout nodes missing");
        const pageRect = page.getBoundingClientRect();
        const cardRect = card.getBoundingClientRect();
        return {
          pageWidth: page.clientWidth,
          pageScrollWidth: page.scrollWidth,
          cardInsidePage: cardRect.left >= pageRect.left && cardRect.right <= pageRect.right,
        };
      });

      expect(geometry.pageScrollWidth).toBe(geometry.pageWidth);
      expect(geometry.cardInsidePage).toBe(true);
    });
  }
});
