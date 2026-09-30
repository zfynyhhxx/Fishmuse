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
});
