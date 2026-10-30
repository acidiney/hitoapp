import { expect, test } from "@playwright/test";
import { providerFixture } from "./fixtures/provider";
import type { Schema } from "../src/shared/schema";

test("a next track waiting for provider preparation is prepared while the current one plays and starts without a gap", async ({
  page,
  baseURL,
}) => {
  test.setTimeout(60_000);
  const fixture = await providerFixture("Deferred", false, true);
  const errors: string[] = [];
  let cleanup: (() => Promise<void>) | null = null;
  let downloads = 0;
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("request", (request) => {
    if (request.method() === "POST" && request.url().endsWith("/download"))
      downloads++;
  });
  try {
    await page.goto("/library");
    await expect(
      page.getByRole("heading", { name: "Your library", exact: true }),
    ).toBeVisible();
    const headers = { "x-hito-request": "1", origin: new URL(baseURL!).origin };
    const offered = await page.request.post("/api/v1/plugins/services", {
      headers,
      data: {
        name: "Deferred service",
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
    cleanup = async () => {
      const removed = await page.request.delete(
        `/api/v1/plugins/${connection.id}`,
        { headers, data: { revision: connection.revision } },
      );
      expect(removed.ok()).toBe(true);
    };
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

    await page.goto("/library?q=Deferred");
    await page.evaluate(() => {
      const events: { type: string; at: number; source: string | null }[] = [];
      (window as unknown as { audioEvents: typeof events }).audioEvents =
        events;
      for (const type of ["playing", "pause", "ended"])
        document.addEventListener(
          type,
          (event) =>
            events.push({
              type,
              at: performance.now(),
              source: (event.target as HTMLAudioElement).getAttribute("src"),
            }),
          true,
        );
    });
    await page
      .getByRole("button", { name: `Play ${saved[0].title}`, exact: true })
      .click();
    const audio = page.locator("audio[data-hito-player]");
    await expect
      .poll(() =>
        audio.evaluate((element: HTMLAudioElement) => element.currentTime),
      )
      .toBeGreaterThan(0.2);
    const first = await audio.evaluate(
      (element: HTMLAudioElement) => element.getAttribute("src")!,
    );

    await expect
      .poll(() => fixture.preparations.begin, { timeout: 20_000 })
      .toBe(1);
    fixture.releasePreparation(fixture.second.id);
    await expect
      .poll(
        () =>
          fixture.requests.filter(
            (request) => request.source === fixture.secondSource,
          ).length,
        { timeout: 30_000 },
      )
      .toBeGreaterThan(0);
    const events = () =>
      page.evaluate(
        () =>
          (
            window as unknown as {
              audioEvents: {
                type: string;
                at: number;
                source: string | null;
              }[];
            }
          ).audioEvents,
      );
    expect(
      await audio.evaluate((element: HTMLAudioElement) => element.paused),
    ).toBe(false);
    expect(
      (await events()).filter(
        (event) => event.type !== "playing" && event.source === first,
      ),
    ).toEqual([]);
    expect(fixture.preparations.begin).toBe(1);

    await audio.evaluate((element: HTMLAudioElement) => {
      element.currentTime = Math.max(0, element.duration - 1.5);
    });
    await expect
      .poll(async () =>
        (await events()).some(
          (event) => event.type === "playing" && event.source !== first,
        ),
      )
      .toBe(true);
    const recorded = await events();
    const ended = recorded
      .filter((event) => event.type === "ended" && event.source === first)
      .pop();
    const resumed = recorded.find(
      (event) => event.type === "playing" && event.source !== first,
    );
    expect(ended && resumed).toBeTruthy();
    expect(resumed!.at - ended!.at).toBeLessThan(1000);
    expect(resumed!.source).not.toBe(first);
    expect(new URL(resumed!.source!, baseURL).pathname).toMatch(
      /^\/api\/v1\/sources\/[a-f0-9-]+\/audio$/,
    );
    expect(fixture.preparations.begin).toBe(1);
    expect(downloads).toBe(0);
    for (const request of fixture.requests) {
      expect(request.authorization).toBe(`Bearer ${fixture.token}`);
      expect(request.cookie).toBeUndefined();
    }
    expect(errors).toEqual([]);
  } finally {
    try {
      await cleanup?.();
    } finally {
      await fixture.close();
    }
  }
});
