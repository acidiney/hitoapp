import { expect, test, type Page } from "@playwright/test";
import { providerFixture } from "./fixtures/provider";
import { openLibrarySection } from "./fixtures/library-sections";
import type { Schema } from "../src/shared/schema";

type Fixture = Awaited<ReturnType<typeof providerFixture>>;

const connected: Schema<"PluginInstallation">[] = [];

test.afterEach(async ({ page, baseURL }) => {
  const headers = { "x-hito-request": "1", origin: new URL(baseURL!).origin };
  for (const connection of connected.splice(0))
    await page.request.delete(`/api/v1/plugins/${connection.id}`, {
      headers,
      data: { revision: connection.revision },
    });
});

async function savedTrack(page: Page, baseURL: string, fixture: Fixture) {
  await page.goto("/library");
  await expect(
    page.getByRole("heading", { name: "Your library", exact: true }),
  ).toBeVisible();
  const headers = { "x-hito-request": "1", origin: new URL(baseURL).origin };
  const offered = await page.request.post("/api/v1/plugins/services", {
    headers,
    data: {
      name: "Fault service",
      base_url: fixture.address,
    } satisfies Schema<"OfferPluginService">,
  });
  expect(offered.status()).toBe(201);
  const service: Schema<"PluginService"> = await offered.json();
  const installed = await page.request.post("/api/v1/plugins", {
    headers,
    data: {
      service_id: service.id,
      service_revision: service.revision,
      bearer_token: fixture.token,
    } satisfies Schema<"InstallPlugin">,
  });
  expect(installed.status()).toBe(201);
  const connection: Schema<"PluginInstallation"> = await installed.json();
  connected.push(connection);
  const adopted = await page.request.post(
    `/api/v1/plugins/${connection.id}/recordings`,
    {
      headers,
      data: {
        installation_revision: connection.revision,
        provider_recording_id: fixture.recording.id,
      } satisfies Schema<"AdoptProviderRecording">,
    },
  );
  expect(adopted.status()).toBe(200);
  const recording: Schema<"Recording"> = await adopted.json();
  await page.goto(`/library?q=${encodeURIComponent(recording.title)}`);
  return recording;
}

const position = (page: Page) =>
  page
    .locator("audio[data-hito-player]")
    .evaluate((element: HTMLAudioElement) => element.currentTime);

async function play(page: Page, title: string) {
  await page
    .getByRole("button", { name: `Play ${title}`, exact: true })
    .click();
}

test("a provider that is slow to send the first byte delays the start but still plays without an error", async ({
  page,
  baseURL,
}) => {
  const fixture = await providerFixture("FaultSlow");
  try {
    const recording = await savedTrack(page, baseURL!, fixture);
    fixture.delayAudio(3000);
    const started = Date.now();
    await play(page, recording.title);
    await expect
      .poll(() => position(page), { timeout: 20_000 })
      .toBeGreaterThan(0.2);
    expect(Date.now() - started).toBeGreaterThanOrEqual(3000);
    await expect(page.getByRole("alert")).toHaveCount(0);
  } finally {
    await fixture.close();
  }
});

test("a connection reset in the middle of a track is recovered from a fresh request and playback passes the break", async ({
  page,
  baseURL,
}) => {
  const fixture = await providerFixture("FaultReset");
  try {
    const recording = await savedTrack(page, baseURL!, fixture);
    fixture.resetAudioAfter(fixture.source, 64_000);
    await play(page, recording.title);
    await expect
      .poll(() => position(page), { timeout: 40_000 })
      .toBeGreaterThan(5);
    const audio = page.locator("audio[data-hito-player]");
    expect(
      await audio.evaluate((element: HTMLAudioElement) => element.paused),
    ).toBe(false);
    expect(
      fixture.requests.filter((request) => request.source === fixture.source)
        .length,
    ).toBeGreaterThanOrEqual(2);
  } finally {
    await fixture.close();
  }
});

test("a provider that ignores a seek range leaves the player playing and unfrozen", async ({
  page,
  baseURL,
}) => {
  const fixture = await providerFixture("FaultRange");
  try {
    const recording = await savedTrack(page, baseURL!, fixture);
    fixture.ignoreRangeRequests();
    await play(page, recording.title);
    await expect.poll(() => position(page)).toBeGreaterThan(0.5);
    const seek = page.getByRole("slider", { name: "Seek", exact: true });
    await seek.focus();
    await seek.press("PageUp");
    await seek.press("PageUp");
    await seek.press("PageUp");
    const sought = await position(page);
    expect(sought).toBeGreaterThan(5);
    await expect
      .poll(() => position(page), { timeout: 15_000 })
      .toBeGreaterThan(sought + 2);
    const audio = page.locator("audio[data-hito-player]");
    expect(
      await audio.evaluate((element: HTMLAudioElement) => element.paused),
    ).toBe(false);
    await expect(page.getByRole("alert")).toHaveCount(0);
  } finally {
    await fixture.close();
  }
});

test("a provider that keeps failing is asked a bounded number of times, says so, and plays again once healed", async ({
  page,
  baseURL,
}) => {
  const fixture = await providerFixture("FaultBurst");
  try {
    const recording = await savedTrack(page, baseURL!, fixture);
    fixture.failSource(fixture.source, 503);
    await play(page, recording.title);
    await expect(page.getByText(/unavailable/i).first()).toBeVisible({
      timeout: 30_000,
    });
    const settled = fixture.requests.length;
    expect(settled).toBeLessThanOrEqual(8);
    await page.waitForTimeout(5000);
    expect(fixture.requests.length).toBe(settled);
    fixture.healSource(fixture.source);
    await play(page, recording.title);
    await expect
      .poll(() => position(page), { timeout: 20_000 })
      .toBeGreaterThan(0.2);
  } finally {
    await fixture.close();
  }
});

test("buffered audio keeps playing through a server outage and the page recovers in place afterwards", async ({
  page,
  baseURL,
}) => {
  test.setTimeout(90_000);
  const fixture = await providerFixture("FaultOutage");
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  let down = false;
  await page.route("**/*", (route) => {
    const url = new URL(route.request().url());
    const talksToHito =
      url.pathname === "/health" ||
      url.pathname.startsWith("/api/v1/") ||
      route.request().headers()["x-inertia"] !== undefined;
    return down && talksToHito
      ? route.abort("connectionrefused")
      : route.continue();
  });
  try {
    const recording = await savedTrack(page, baseURL!, fixture);
    await play(page, recording.title);
    const audio = page.locator("audio[data-hito-player]");
    await expect
      .poll(() =>
        audio.evaluate(
          (element: HTMLAudioElement) =>
            element.buffered.length > 0 &&
            element.buffered.end(element.buffered.length - 1) >=
              element.duration - 0.5,
        ),
      )
      .toBe(true);
    const source = await audio.evaluate(
      (element: HTMLAudioElement) => element.src,
    );
    const before = await position(page);
    down = true;
    await openLibrarySection(page, "Albums");
    const notice = page.getByRole("status").filter({ hasText: "Cannot reach" });
    await expect(notice).toBeVisible();
    await expect
      .poll(() => position(page), { timeout: 15_000 })
      .toBeGreaterThan(before + 3);
    expect(
      await audio.evaluate((element: HTMLAudioElement) => element.paused),
    ).toBe(false);
    down = false;
    await expect(notice).toBeHidden({ timeout: 20_000 });
    expect(
      await audio.evaluate((element: HTMLAudioElement) => element.src),
    ).toBe(source);
    expect(
      await audio.evaluate((element: HTMLAudioElement) => element.paused),
    ).toBe(false);
    expect(errors).toEqual([]);
  } finally {
    await fixture.close();
  }
});
