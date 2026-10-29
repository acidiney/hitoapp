import { expect, test } from "@playwright/test";
import { providerFixture } from "./fixtures/provider";
import type { components } from "../src/generated";
import { openLibrarySection } from "./fixtures/library-sections";
type Schema = components["schemas"];

test("selected provider audio plays and seeks in the library without exposing its private connection", async ({
  page,
  baseURL,
}) => {
  const fixture = await providerFixture();
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
        name: "Relay service",
        base_url: fixture.address,
      } satisfies Schema["OfferPluginService"],
    });
    expect(offered.status()).toBe(201);
    const service: Schema["PluginService"] = await offered.json();
    const installed = await page.request.post("/api/v1/plugins", {
      headers,
      data: {
        service_id: service.id,
        service_revision: service.revision,
        bearer_token: fixture.token,
      } satisfies Schema["InstallPlugin"],
    });
    expect(installed.status()).toBe(201);
    const connection: Schema["PluginInstallation"] = await installed.json();
    const results = await page.request.post(
      `/api/v1/plugins/${connection.id}/search`,
      {
        headers,
        data: {
          installation_revision: connection.revision,
          query: "Relay",
          limit: 10,
        } satisfies Schema["ProviderSearch"],
      },
    );
    expect(results.status()).toBe(200);
    const found: Schema["SearchPage"] = await results.json();
    expect(found.items.map((item) => item.id)).toEqual([fixture.recording.id]);
    const adopted = await page.request.post(
      `/api/v1/plugins/${connection.id}/recordings`,
      {
        headers,
        data: {
          installation_revision: connection.revision,
          provider_recording_id: found.items[0].id,
        } satisfies Schema["AdoptProviderRecording"],
      },
    );
    expect(adopted.status()).toBe(200);
    const recording: Schema["Recording"] = await adopted.json();
    expect(recording.id).not.toBe(fixture.recording.id);
    await page.goto(`/library?q=${encodeURIComponent(recording.title)}`);
    await page
      .getByRole("button", { name: `Play ${recording.title}`, exact: true })
      .click();
    const audio = page.locator("audio[data-hito-player]");
    await expect
      .poll(() =>
        audio.evaluate((element: HTMLAudioElement) => element.currentTime),
      )
      .toBeGreaterThan(0.2);
    const source = await audio.evaluate(
      (element: HTMLAudioElement) => element.src,
    );
    expect(new URL(source).pathname).toMatch(
      /^\/api\/v1\/sources\/[a-f0-9-]+\/audio$/,
    );
    expect(source).not.toContain("provider:");
    const seek = page.getByRole("slider", { name: "Seek", exact: true });
    await seek.focus();
    await seek.press("PageUp");
    await expect
      .poll(() =>
        audio.evaluate((element: HTMLAudioElement) => element.currentTime),
      )
      .toBeGreaterThan(5);
    const before = await audio.evaluate(
      (element: HTMLAudioElement) => element.currentTime,
    );
    await openLibrarySection(page, "History");
    await expect
      .poll(() =>
        audio.evaluate((element: HTMLAudioElement) => element.currentTime),
      )
      .toBeGreaterThan(before);
    expect(
      await audio.evaluate((element: HTMLAudioElement) => element.src),
    ).toBe(source);
    expect(fixture.requests.length).toBeGreaterThan(0);
    for (const request of fixture.requests) {
      expect(request.authorization).toBe(`Bearer ${fixture.token}`);
      expect(request.cookie).toBeUndefined();
      expect(request.source).toBe(fixture.source);
    }
    const removed = await page.request.delete(
      `/api/v1/plugins/${connection.id}`,
      { headers, data: { revision: connection.revision } },
    );
    expect(removed.ok()).toBe(true);
    await page.goto(`/library?q=${encodeURIComponent(recording.title)}`);
    await expect(
      page.getByRole("button", {
        name: `Play ${recording.title}`,
        exact: true,
      }),
    ).toBeEnabled();
    expect(errors).toEqual([]);
  } finally {
    await fixture.close();
  }
});
