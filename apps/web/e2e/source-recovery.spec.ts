import { expect, test } from "@playwright/test";
import { providerFixture } from "./fixtures/provider";
import type { Schema } from "../src/shared/schema";
import { openTrackEditors } from "./fixtures/track-panels";
import { trackTitleLink } from "./fixtures/track-row";

test("expired seek audio refreshes near its position and unavailable tracks can be checked without interrupting playback", async ({
  page,
  baseURL,
}) => {
  const fixture = await providerFixture("Recovery");
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  try {
    await page.goto("/library");
    await expect(
      page.getByRole("heading", { name: "Your library", exact: true }),
    ).toBeVisible();
    const headers = { "x-hito-request": "1", origin: new URL(baseURL!).origin };
    const offered = await page.request.post("/api/v1/plugins/services", {
      headers,
      data: {
        name: "Recovery service",
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
    fixture.setSources(fixture.second.id, [
      { id: fixture.secondSource, label: "Preparing", ready: false },
    ]);
    const saved: Schema<"Recording">[] = [];
    for (const item of [fixture.recording, fixture.second]) {
      const response = await page.request.post(
        `/api/v1/plugins/${connection.id}/recordings`,
        {
          headers,
          data: {
            installation_revision: connection.revision,
            provider_recording_id: item.id,
          } satisfies Schema<"AdoptProviderRecording">,
        },
      );
      expect(response.status()).toBe(200);
      saved.push(await response.json());
    }
    expect(saved[1].available).toBe(false);
    const tail = fixture.holdAudioTail(fixture.source);
    await page.goto("/library?q=Recovery");
    await page
      .getByRole("button", { name: `Play ${saved[0].title}`, exact: true })
      .click();
    const audio = page.locator("audio[data-hito-player]");
    await expect.poll(() => tail.entered).toBe(true);
    await expect
      .poll(() =>
        audio.evaluate((element: HTMLAudioElement) => element.currentTime),
      )
      .toBeGreaterThan(0.2);
    const original = await audio.evaluate(
      (element: HTMLAudioElement) => element.src,
    );
    const replacement = `${fixture.source}/replacement`;
    fixture.setSources(fixture.recording.id, [
      { id: replacement, label: "Replacement", ready: true },
    ]);
    fixture.failSource(fixture.source);
    const seek = page.getByRole("slider", { name: "Seek", exact: true });
    await seek.focus();
    await seek.press("PageUp");
    await seek.press("PageUp");
    await seek.press("PageUp");
    await expect
      .poll(() => audio.evaluate((element: HTMLAudioElement) => element.src))
      .not.toBe(original);
    await expect
      .poll(() =>
        audio.evaluate((element: HTMLAudioElement) => element.currentTime),
      )
      .toBeGreaterThan(8);
    await expect
      .poll(() => audio.evaluate((element: HTMLAudioElement) => element.paused))
      .toBe(false);
    expect(
      fixture.requests.some((request) => request.source === replacement),
    ).toBe(true);
    expect(
      fixture.sourceRequests.filter((id) => id === fixture.recording.id),
    ).toHaveLength(2);
    const recovered = await audio.evaluate(
      (element: HTMLAudioElement) => element.src,
    );
    await trackTitleLink(page.locator("main"), saved[1].title).click();
    await expect(
      page.getByRole("heading", {
        name: saved[1].title,
        level: 1,
        exact: true,
      }),
    ).toBeVisible();
    await openTrackEditors(page);
    const title = page.getByLabel("Track title", { exact: true });
    await title.fill("Keep this unsaved draft");
    fixture.setSources(fixture.second.id, [
      { id: fixture.secondSource, label: "Ready now", ready: true },
    ]);
    await page
      .getByRole("button", { name: "Check availability", exact: true })
      .click();
    await expect(
      page.getByText("Audio is ready to play.", { exact: true }),
    ).toBeVisible();
    await expect(title).toHaveValue("Keep this unsaved draft");
    expect(
      await audio.evaluate((element: HTMLAudioElement) => element.src),
    ).toBe(recovered);
    await page.screenshot({
      path: "test-results/source-availability-desktop.png",
    });
    await page.setViewportSize({ width: 390, height: 844 });
    const panel = page.getByRole("region", { name: "Audio availability" });
    await panel.scrollIntoViewIfNeeded();
    await page.screenshot({
      path: "test-results/source-availability-mobile.png",
    });
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth > window.innerWidth,
      ),
    ).toBe(false);
    const play = page.getByRole("button", { name: "Play track", exact: true });
    await page.evaluate(() =>
      window.scrollTo(0, document.documentElement.scrollHeight),
    );
    const dock = await page
      .getByRole("contentinfo", { name: "Audio player" })
      .boundingBox();
    for (const control of [play, panel]) {
      const bounds = await control.boundingBox();
      expect(bounds && dock && bounds.y + bounds.height <= dock.y).toBe(true);
    }
    await play.click();
    await expect
      .poll(() => audio.evaluate((element: HTMLAudioElement) => element.src))
      .not.toBe(recovered);
    await expect
      .poll(() =>
        audio.evaluate((element: HTMLAudioElement) => element.currentTime),
      )
      .toBeGreaterThan(0.2);
    expect(
      fixture.requests.some(
        (request) => request.source === fixture.secondSource,
      ),
    ).toBe(true);
    for (const request of fixture.requests) {
      expect(request.authorization).toBe(`Bearer ${fixture.token}`);
      expect(request.cookie).toBeUndefined();
    }
    expect(errors).toEqual([]);
  } finally {
    await fixture.close();
  }
});
