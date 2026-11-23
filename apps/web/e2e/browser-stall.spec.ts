import { expect, test, type Page, type Route } from "@playwright/test";
import { enterAdmin, sharedAdminSession } from "./fixtures/admin-session";
import { seedFolder } from "./fixtures/playback-status";

const title = "Fixture 440 Hz";
const audioPath = "**/api/v1/sources/*/audio";
const bytesPerSecond = 32_000;
const deliveredBytes = 32_768 * 8;
const tailBytes = deliveredBytes + bytesPerSecond * 10;

const dock = (page: Page) =>
  page.getByRole("contentinfo", { name: "Audio player", exact: true });
const status = (page: Page) => dock(page).locator(".playback-status span");
const audio = (page: Page) => page.locator("audio[data-hito-player]");

class HeldAudio {
  ranges: string[] = [];
  private held: Route | null = null;
  private enter!: () => void;
  entered = new Promise<void>((resolve) => {
    this.enter = resolve;
  });

  handle = async (route: Route) => {
    const headers = route.request().headers();
    const range = headers.range ?? "";
    this.ranges.push(range);
    const start = Number(/^bytes=(\d+)-/.exec(range)?.[1] ?? 0);
    if (this.held !== null || start >= tailBytes) {
      await route.continue();
      return;
    }
    if (start >= deliveredBytes) {
      this.held = route;
      this.enter();
      return;
    }
    const response = await route.fetch({
      headers: { ...headers, range: `bytes=${start}-${deliveredBytes - 1}` },
    });
    await route.fulfill({ response });
  };

  async close() {
    try {
      await this.held?.abort();
    } catch {}
  }
}

async function element(page: Page) {
  return audio(page).evaluate((media: HTMLAudioElement) => ({
    time: media.currentTime,
    paused: media.paused,
    starved: media.readyState < 3,
    tagged: media.dataset.stallProbe === "kept",
  }));
}

test("audio held mid-track shows Buffering and stops the system clock, then reopens on the same element where it stopped", async ({
  page,
}) => {
  await page.clock.install();
  const held = new HeldAudio();
  await page.route(audioPath, held.handle);
  try {
    if (!sharedAdminSession) await enterAdmin(page);
    await page.emulateMedia({ colorScheme: "dark" });
    await seedFolder(page, "Music");
    await page.goto("/library?q=Hz&sort=title");
    await page.getByRole("button", { name: "List view", exact: true }).click();
    const row = page
      .getByRole("table", { name: "Tracks", exact: true })
      .locator("tbody tr")
      .filter({ hasText: title });
    await row
      .getByRole("button", { name: `Play ${title}`, exact: true })
      .click();

    await held.entered;
    await expect
      .poll(
        async () => {
          const media = await element(page);
          return !media.paused && media.starved && media.time > 4;
        },
        { timeout: 20_000 },
      )
      .toBe(true);
    const stalledAt = (await element(page)).time;
    await audio(page).evaluate((media: HTMLAudioElement) => {
      media.dataset.stallProbe = "kept";
      const probe = { samples: [] as number[], loads: 0 };
      (window as unknown as { stallProbe: typeof probe }).stallProbe = probe;
      media.addEventListener("loadstart", () => probe.loads++);
      media.addEventListener("timeupdate", () => {
        if (media.readyState >= 2) probe.samples.push(media.currentTime);
      });
    });
    await page.screenshot({ path: "test-results/web-stall-desktop.png" });
    await page.setViewportSize({ width: 390, height: 844 });
    await page.screenshot({ path: "test-results/web-stall-phone.png" });
    await page
      .getByRole("button", { name: "Expand player", exact: true })
      .click();
    await expect(dock(page)).toBeHidden();
    await page.waitForFunction(() =>
      document
        .getAnimations()
        .every(
          (animation) =>
            animation.playState !== "running" ||
            animation.effect?.getComputedTiming().endTime === Infinity,
        ),
    );
    await page.screenshot({
      path: "test-results/web-stall-phone-now-playing.png",
    });
    await page.keyboard.press("Escape");
    await page.setViewportSize({ width: 1440, height: 1000 });

    await expect(status(page)).toHaveText("Buffering…");
    expect(
      await page.evaluate(() => navigator.mediaSession.playbackState),
    ).toBe("paused");
    await expect(
      dock(page).getByRole("button", { name: "Pause", exact: true }),
    ).toBeEnabled();
    const rangesBeforeDeadline = held.ranges.length;

    await page.clock.runFor(15_000);

    await expect(status(page)).toHaveCount(0);
    await expect
      .poll(async () => (await element(page)).time)
      .toBeGreaterThan(stalledAt + 1);
    const after = await element(page);
    expect(after.tagged).toBe(true);
    expect(after.paused).toBe(false);
    expect(held.ranges.length).toBeGreaterThan(rangesBeforeDeadline);
    const probe = await page.evaluate(
      () =>
        (
          window as unknown as {
            stallProbe: { samples: number[]; loads: number };
          }
        ).stallProbe,
    );
    expect(probe.loads).toBeGreaterThan(0);
    expect(probe.samples.length).toBeGreaterThan(0);
    expect(Math.min(...probe.samples)).toBeGreaterThanOrEqual(stalledAt - 0.5);
    expect(
      await page.evaluate(() => navigator.mediaSession.playbackState),
    ).toBe("playing");
    await expect(dock(page).locator(".now-playing")).toContainText(title);
  } finally {
    await held.close();
    await page.unroute(audioPath, held.handle);
  }
});
