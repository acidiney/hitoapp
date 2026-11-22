import { expect, test, type Page } from "@playwright/test";
import { providerFixture } from "./fixtures/provider";
import type { Schema } from "../src/shared/schema";

const playerPath = (page: Page) =>
  page
    .locator("audio[data-hito-player]")
    .evaluate((element: HTMLAudioElement) =>
      element.getAttribute("src")
        ? new URL(element.getAttribute("src")!, location.href).pathname
        : null,
    );

const position = (page: Page) =>
  page
    .locator("audio[data-hito-player]")
    .evaluate((element: HTMLAudioElement) =>
      element.paused ? 0 : element.currentTime,
    );

test("the web player announces the next track and then plays it from the copy the server saved", async ({
  page,
  baseURL,
}) => {
  test.setTimeout(120_000);
  const prefix = `Copies ${Date.now() % 1_000_000}`;
  const fixture = await providerFixture(prefix);
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  const headers = {
    "x-hito-request": "1",
    origin: new URL(baseURL!).origin,
  };
  let cleanup: (() => Promise<void>) | null = null;
  try {
    const offered = await page.request.post("/api/v1/plugins/services", {
      headers,
      data: {
        name: `${prefix} service`,
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
    const [first, second] = saved;
    const before: Schema<"AutomaticCopies"> = await (
      await page.request.get("/api/v1/account/automatic-copies")
    ).json();
    expect(before.keep_played_tracks).toBe(true);
    const secondSources = async () =>
      (await (
        await page.request.get(
          `/api/v1/recordings/${second.id}/playback-sources`,
        )
      ).json()) as Schema<"Source">[];
    expect(
      (await secondSources()).some((source) => source.provider === "download"),
    ).toBe(false);

    const announced = page.waitForRequest(
      (request) =>
        request.method() === "POST" &&
        new URL(request.url()).pathname === "/api/v1/playback/upcoming" &&
        request.postDataJSON()?.current === first.id,
    );
    await page.goto(`/library?q=${encodeURIComponent(prefix)}&sort=title`);
    await page
      .getByRole("button", { name: `Play ${first.title}`, exact: true })
      .click();
    const announcement = await announced;
    const body: Schema<"UpcomingRecordings"> = announcement.postDataJSON();
    expect(body.client).toMatch(/^web-[A-Za-z0-9]{12}$/);
    expect(body.recording_ids).toEqual([second.id]);
    expect(announcement.headers()["x-hito-request"]).toBe("1");
    expect((await announcement.response())?.status()).toBe(204);
    await expect
      .poll(() => position(page), { timeout: 15_000 })
      .toBeGreaterThan(0.2);

    let copy: Schema<"Source"> | undefined;
    await expect
      .poll(
        async () => {
          const sources = await secondSources();
          copy = sources.find(
            (source) => source.provider === "download" && source.ready,
          );
          return !!copy && sources[0].id === copy.id;
        },
        { timeout: 60_000 },
      )
      .toBe(true);
    const after: Schema<"AutomaticCopies"> = await (
      await page.request.get("/api/v1/account/automatic-copies")
    ).json();
    expect(after.used_bytes).toBeGreaterThan(before.used_bytes);

    const fromProvider = fixture.requests.length;
    await page
      .locator("audio[data-hito-player]")
      .evaluate((element: HTMLAudioElement) => {
        element.currentTime = Math.max(0, element.duration - 1);
      });
    await expect
      .poll(() => playerPath(page), { timeout: 30_000 })
      .toBe(`/api/v1/sources/${encodeURIComponent(copy!.id)}/audio`);
    await expect
      .poll(() => position(page), { timeout: 15_000 })
      .toBeGreaterThan(0.2);
    expect(
      fixture.requests
        .slice(fromProvider)
        .filter((request) => request.source === fixture.secondSource),
    ).toEqual([]);
    await expect(page.getByLabel("Audio player")).toContainText(second.title);
    expect(errors).toEqual([]);
  } finally {
    try {
      await cleanup?.();
    } finally {
      await fixture.close();
    }
  }
});
