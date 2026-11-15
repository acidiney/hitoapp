import { randomUUID } from "node:crypto";
import { expect, test, type Page, type Route } from "@playwright/test";
import type { Schema } from "../src/shared/schema";
import { enterAdmin, sharedAdminSession } from "./fixtures/admin-session";
import { openTrackMenu } from "./fixtures/track-row";
import { expectReadableNotice, seedFolder } from "./fixtures/playback-status";

const first = "Fixture 440 Hz";
const second = "Fixture 660 Hz";
const outageMessage =
  "An audio service is unavailable. Select the track again to retry.";
const discoveryPath =
  /^\/api\/v1\/recordings\/([0-9a-f-]+)\/(playback-sources|sources\/refresh|sources\/resolve)$/;

type Step = "cached" | "refresh" | "resolve";

const dock = (page: Page) =>
  page.getByRole("contentinfo", { name: "Audio player", exact: true });
const status = (page: Page) => dock(page).locator(".playback-status span");
const audio = (page: Page) => page.locator("audio[data-hito-player]");

class OwnedDiscovery {
  target: string | null = null;
  passes = 0;
  resolves: string[] = [];
  resolvePasses: number[] = [];
  answered: string[] = [];
  private holds = new Map<string, (route: Route) => void>();
  private held = new Map<string, Route>();

  constructor(private failures: number) {}

  hold(step: Step, pass: number) {
    return new Promise<void>((resolve) => {
      this.holds.set(`${step}:${pass}`, () => resolve());
    });
  }

  async release(step: Step, pass: number) {
    const key = `${step}:${pass}`;
    const route = this.held.get(key);
    this.held.delete(key);
    if (!route) throw new Error(`No held ${key} request`);
    try {
      await this.answer(route, step, pass);
    } catch {}
  }

  async close() {
    for (const route of this.held.values())
      try {
        await route.abort();
      } catch {}
    this.held.clear();
  }

  handle = async (route: Route) => {
    const url = new URL(route.request().url());
    const match = discoveryPath.exec(url.pathname);
    if (!match) return route.continue();
    const [, recording, kind] = match;
    if (this.target === null && kind === "playback-sources")
      this.target = recording;
    if (recording !== this.target) return route.continue();
    const step: Step =
      kind === "playback-sources"
        ? "cached"
        : kind === "sources/refresh"
          ? "refresh"
          : "resolve";
    if (step === "cached") this.passes++;
    const pass = this.passes;
    if (step === "resolve") this.resolvePasses.push(pass);
    if (step === "resolve")
      this.resolves.push(
        (route.request().postDataJSON() as Schema<"ResolveSource">).request_id,
      );
    const key = `${step}:${pass}`;
    const entered = this.holds.get(key);
    if (entered) {
      this.holds.delete(key);
      this.held.set(key, route);
      entered(route);
      return;
    }
    await this.answer(route, step, pass);
  };

  private async answer(route: Route, step: Step, pass: number) {
    if (pass > this.failures) return route.continue();
    if (step !== "resolve") return route.fulfill({ json: [] });
    const body = route.request().postDataJSON() as Schema<"ResolveSource">;
    this.answered.push("provider_unavailable");
    return route.fulfill({
      json: {
        request_id: body.request_id,
        resolution_id: randomUUID(),
        recording_id: this.target!,
        outcome: "provider_unavailable",
        sources: [],
      } satisfies Schema<"SourceResolution">,
    });
  }
}

async function openLibrary(page: Page) {
  if (!sharedAdminSession) await enterAdmin(page);
  await page.emulateMedia({ colorScheme: "dark" });
  await seedFolder(page, "Music");
  await seedFolder(page, "Archive");
  await page.goto("/library?q=Hz&sort=title");
  await expect(
    page.getByRole("heading", { name: "Your library", exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: "List view", exact: true }).click();
  const table = page.getByRole("table", { name: "Tracks", exact: true });
  for (const title of [first, second])
    await expect(
      table
        .locator("tbody tr")
        .filter({ hasText: title })
        .getByRole("button", { name: `Play ${title}`, exact: true }),
    ).toBeEnabled();
  return {
    firstRow: table.locator("tbody tr").filter({ hasText: first }),
    secondRow: table.locator("tbody tr").filter({ hasText: second }),
  };
}

async function heard(page: Page) {
  await expect
    .poll(
      () =>
        audio(page).evaluate(
          (element: HTMLAudioElement) =>
            !element.paused &&
            element.error === null &&
            element.currentTime > 0.3 &&
            new URL(element.src, location.href).pathname.startsWith(
              "/api/v1/sources/",
            ),
        ),
      { timeout: 10_000 },
    )
    .toBe(true);
}

async function audioSource(page: Page) {
  return audio(page).evaluate((element: HTMLAudioElement) =>
    element.getAttribute("src"),
  );
}

for (const [name, viewport] of [
  ["desktop", { width: 1440, height: 900 }],
  ["phone", { width: 390, height: 844 }],
] as const)
  test(`an unavailable first pass keeps the track and shows its retry instead of skipping, ${name}`, async ({
    page,
  }) => {
    const discovery = new OwnedDiscovery(Number.POSITIVE_INFINITY);
    const { firstRow, secondRow } = await openLibrary(page);
    await page.setViewportSize(viewport);
    await page.route("**/api/v1/recordings/**", discovery.handle);
    try {
      const pass1 = discovery.hold("resolve", 1);
      await firstRow
        .getByRole("button", { name: `Play ${first}`, exact: true })
        .click();
      await pass1;
      await (await openTrackMenu(secondRow, second))
        .getByRole("menuitem", { name: "Add to queue" })
        .click();
      void discovery.hold("cached", 2);
      await discovery.release("resolve", 1);
      expect(discovery.answered).toEqual(["provider_unavailable"]);
      await expect(status(page)).toHaveText(
        /^(Trying again \(2 of 3\)…|An audio service is unavailable\. Select the track again to retry\.)$/,
      );
      const observed = {
        status: await status(page).textContent(),
        playing: await dock(page).locator(".now-playing").textContent(),
        source: await audioSource(page),
      };
      await page.screenshot({
        path: `test-results/browser-retry-${name}-first-failure.png`,
      });
      expect(observed, JSON.stringify(observed)).toEqual({
        status: "Trying again (2 of 3)…",
        playing: expect.stringContaining(first),
        source: null,
      });
      await expectReadableNotice(page, name === "phone");
    } finally {
      await discovery.close();
      await page.unroute("**/api/v1/recordings/**", discovery.handle);
    }
  });

for (const [name, viewport] of [
  ["desktop", { width: 1440, height: 900 }],
  ["phone", { width: 390, height: 844 }],
] as const)
  test(`a track unavailable twice shows each retry and plays on the third pass, ${name}`, async ({
    page,
  }) => {
    const discovery = new OwnedDiscovery(2);
    const { firstRow } = await openLibrary(page);
    await page.setViewportSize(viewport);
    const phone = name === "phone";
    await page.route("**/api/v1/recordings/**", discovery.handle);
    try {
      const pass2 = discovery.hold("resolve", 2);
      await firstRow
        .getByRole("button", { name: `Play ${first}`, exact: true })
        .click();
      await pass2;
      await expect(status(page)).toHaveText("Trying again (2 of 3)…");
      await expectReadableNotice(page, phone);
      await page.screenshot({
        path: `test-results/browser-retry-${name}-2-of-3.png`,
      });
      const third = discovery.hold("cached", 3);
      await discovery.release("resolve", 2);
      await third;
      await expect(status(page)).toHaveText("Trying again (3 of 3)…");
      await expectReadableNotice(page, phone);
      await page.screenshot({
        path: `test-results/browser-retry-${name}-3-of-3.png`,
      });
      await discovery.release("cached", 3);
      await heard(page);
      await expect(status(page)).toHaveCount(0);
      await expect(dock(page).locator(".now-playing")).toContainText(first);
      expect(discovery.passes).toBe(3);
      expect(discovery.resolvePasses).toEqual([1, 2]);
      expect(new Set(discovery.resolves).size).toBe(2);
      await page.screenshot({
        path: `test-results/browser-retry-${name}-playing.png`,
      });
    } finally {
      await discovery.close();
      await page.unroute("**/api/v1/recordings/**", discovery.handle);
    }
  });

test("Pause during a held retry clears it, a late reply cannot restore audio, and Play searches again", async ({
  page,
}) => {
  await page.clock.install();
  const discovery = new OwnedDiscovery(2);
  const { firstRow } = await openLibrary(page);
  await page.route("**/api/v1/recordings/**", discovery.handle);
  try {
    const held = discovery.hold("resolve", 2);
    await firstRow
      .getByRole("button", { name: `Play ${first}`, exact: true })
      .click();
    await held;
    await expect(status(page)).toHaveText("Trying again (2 of 3)…");
    const pause = dock(page).getByRole("button", {
      name: "Pause",
      exact: true,
    });
    await expect(pause).toBeEnabled();
    await pause.click();
    await expect(status(page)).toHaveCount(0);
    await expect(
      dock(page).getByRole("button", { name: "Play", exact: true }),
    ).toBeEnabled();
    await page.screenshot({ path: "test-results/browser-retry-paused.png" });
    await discovery.release("resolve", 2);
    await page.clock.runFor(16_000);
    expect(discovery.passes).toBe(2);
    expect(discovery.resolves).toHaveLength(2);
    await expect(status(page)).toHaveCount(0);
    expect(await audioSource(page)).toBeNull();
    await expect(dock(page).locator(".now-playing")).toContainText(first);
    await dock(page).getByRole("button", { name: "Play", exact: true }).click();
    await heard(page);
    expect(discovery.passes).toBe(3);
    await expect(dock(page).locator(".now-playing")).toContainText(first);
  } finally {
    await discovery.close();
    await page.unroute("**/api/v1/recordings/**", discovery.handle);
  }
});

test("Next during a held retry plays the next track and the late reply cannot bring the old one back", async ({
  page,
}) => {
  await page.clock.install();
  const discovery = new OwnedDiscovery(Number.POSITIVE_INFINITY);
  const { firstRow, secondRow } = await openLibrary(page);
  await page.route("**/api/v1/recordings/**", discovery.handle);
  try {
    const held = discovery.hold("resolve", 2);
    await firstRow
      .getByRole("button", { name: `Play ${first}`, exact: true })
      .click();
    await expect(status(page)).toHaveText("Trying again (2 of 3)…");
    await (await openTrackMenu(secondRow, second))
      .getByRole("menuitem", { name: "Add to queue" })
      .click();
    await held;
    await dock(page)
      .getByRole("button", { name: "Next track", exact: true })
      .click();
    await heard(page);
    await expect(dock(page).locator(".now-playing")).toContainText(second);
    const playing = await audioSource(page);
    await discovery.release("resolve", 2);
    await page.clock.runFor(16_000);
    expect(discovery.resolves).toHaveLength(2);
    expect(await audioSource(page)).toBe(playing);
    await expect(dock(page).locator(".now-playing")).toContainText(second);
    await expect(status(page)).toHaveCount(0);
  } finally {
    await discovery.close();
    await page.unroute("**/api/v1/recordings/**", discovery.handle);
  }
});

test("three unavailable passes advance to the next track exactly once", async ({
  page,
}) => {
  await page.clock.install();
  const discovery = new OwnedDiscovery(Number.POSITIVE_INFINITY);
  const { firstRow, secondRow } = await openLibrary(page);
  await page.route("**/api/v1/recordings/**", discovery.handle);
  try {
    const held = discovery.hold("resolve", 1);
    await firstRow
      .getByRole("button", { name: `Play ${first}`, exact: true })
      .click();
    await held;
    await (await openTrackMenu(secondRow, second))
      .getByRole("menuitem", { name: "Add to queue" })
      .click();
    await discovery.release("resolve", 1);
    await heard(page);
    await expect(dock(page).locator(".now-playing")).toContainText(second);
    await expect(status(page)).toHaveText(outageMessage);
    expect(discovery.passes).toBe(3);
    expect(new Set(discovery.resolves).size).toBe(3);
    await page.clock.runFor(16_000);
    expect(discovery.passes).toBe(3);
    await expect(dock(page).locator(".now-playing")).toContainText(second);
  } finally {
    await discovery.close();
    await page.unroute("**/api/v1/recordings/**", discovery.handle);
  }
});
