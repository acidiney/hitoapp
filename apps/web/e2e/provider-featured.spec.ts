import { randomUUID } from "node:crypto";
import { expect, test } from "@playwright/test";
import { featuredShelfFixture } from "./fixtures/featured-shelf";
import type { Schema } from "../src/shared/schema";

test("Discover shows music featured by a connected service and every card opens and plays", async ({
  page,
  baseURL,
}) => {
  const fixture = await featuredShelfFixture();
  const headers = { "x-hito-request": "1", origin: new URL(baseURL!).origin };
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  const failed: string[] = [];
  page.on("response", (response) => {
    if (response.status() >= 400)
      failed.push(`${response.status()} ${new URL(response.url()).pathname}`);
  });
  let connection: Schema<"PluginInstallation"> | null = null;
  let service: Schema<"PluginService"> | null = null;
  const automation = (value: boolean) =>
    page.request.put("/api/v1/metadata/automation", {
      headers,
      data: { automatic_service_search: value },
    });
  try {
    await page.goto("/discover");
    await expect(
      page.getByRole("heading", { level: 1, name: "Discover" }),
    ).toBeVisible();
    await expect(
      page.getByRole("heading", { name: "Search your services" }),
    ).toBeVisible();
    await expect(
      page.getByRole("region", { name: "From your services" }),
    ).toHaveCount(0);

    const offered = await page.request.post("/api/v1/plugins/services", {
      headers,
      data: {
        name: "Featured fixture",
        base_url: fixture.address,
      } satisfies Schema<"OfferPluginService">,
    });
    expect(offered.status()).toBe(201);
    service = await offered.json();
    const installed = await page.request.post("/api/v1/plugins", {
      headers,
      data: {
        service_id: service!.id,
        service_revision: service!.revision,
        bearer_token: fixture.token,
      } satisfies Schema<"InstallPlugin">,
    });
    expect(installed.status()).toBe(201);
    connection = await installed.json();

    await page.goto("/discover");
    const shelf = page.getByRole("region", { name: "From your services" });
    await expect(shelf.getByRole("listitem")).toHaveCount(2);
    await expect(shelf).toContainText(`${fixture.prefix} morning mix`);
    await expect(shelf).toContainText("Playlist · 2 tracks");
    await expect(shelf).toContainText("New release · 2 tracks");
    const cover = shelf.locator("img").first();
    await expect
      .poll(() =>
        cover.evaluate((image: HTMLImageElement) => image.naturalWidth),
      )
      .toBeGreaterThan(0);
    await expect(shelf.locator("img")).toHaveCount(1);
    await page.screenshot({
      path: "/workspace/test-results/provider-featured-desktop.png",
    });

    await page.setViewportSize({ width: 390, height: 844 });
    await shelf.scrollIntoViewIfNeeded();
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth > innerWidth,
      ),
    ).toBe(false);
    const play = shelf.getByRole("button", {
      name: `Play ${fixture.prefix} morning mix`,
    });
    const box = await play.boundingBox();
    const dock = await page
      .getByRole("contentinfo", { name: "Audio player" })
      .boundingBox();
    expect(box!.width).toBeGreaterThanOrEqual(44);
    expect(box!.x + box!.width).toBeLessThanOrEqual(390);
    expect(dock).not.toBeNull();
    await page.screenshot({
      path: "/workspace/test-results/provider-featured-mobile.png",
    });
    await page.setViewportSize({ width: 1440, height: 1000 });

    await play.click();
    await expect(
      page.getByRole("contentinfo", { name: "Audio player" }),
    ).toContainText(`${fixture.prefix} playlist-mix one`, { timeout: 30000 });
    await expect(page).toHaveURL(/\/discover$/);
    await expect(page.getByRole("status")).toContainText(
      `Playing ${fixture.prefix} morning mix`,
    );
    const playlists = await (
      await page.request.get("/api/v1/playlists")
    ).json();
    expect(playlists.map((item: { name: string }) => item.name)).toContain(
      `${fixture.prefix} morning mix`,
    );

    await play.click();
    await expect(page.getByRole("status")).toContainText("Playing");
    const again: { name: string }[] = await (
      await page.request.get("/api/v1/playlists")
    ).json();
    expect(
      again.filter((item) => item.name === `${fixture.prefix} morning mix`),
    ).toHaveLength(1);

    await shelf
      .getByRole("button", { name: `Open ${fixture.prefix} fresh album` })
      .click();
    await expect(page).toHaveURL(/\/playlists\/[0-9a-f-]{36}$/);
    await expect(
      page.getByRole("heading", {
        level: 1,
        name: `${fixture.prefix} fresh album`,
      }),
    ).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Play playlist" }),
    ).toBeEnabled();
    await page.getByRole("button", { name: "Play playlist" }).click();
    await expect(
      page.getByRole("contentinfo", { name: "Audio player" }),
    ).toContainText(`${fixture.prefix} album-fresh one`, { timeout: 30000 });

    const asked = fixture.featuredRequests();
    await page.goto("/discover");
    await expect(shelf.getByRole("listitem")).toHaveCount(2);
    expect(fixture.featuredRequests().length).toBe(asked.length);
    await expect
      .poll(() =>
        shelf
          .locator("img")
          .first()
          .evaluate((image: HTMLImageElement) => image.naturalWidth),
      )
      .toBeGreaterThan(0);

    expect((await automation(false)).ok()).toBe(true);
    await page.goto("/discover");
    await expect(
      page.getByRole("heading", { name: "Search your services" }),
    ).toBeVisible();
    await expect(
      page.getByRole("region", { name: "From your services" }),
    ).toHaveCount(0);
    await expect(page.getByRole("alert")).toHaveCount(0);
    expect(fixture.featuredRequests().length).toBe(asked.length);
    expect((await automation(true)).ok()).toBe(true);

    expect(asked).toHaveLength(1);
    expect(asked[0].body).toEqual({ limit: 12 });
    for (const request of fixture.requests) {
      expect(request.authorization).toBe(`Bearer ${fixture.token}`);
      expect(request.cookie).toBeUndefined();
    }
    expect(errors).toEqual([]);
    const expected = [
      /^404 \/api\/v1\/(recordings|releases|playlists)\/[0-9a-f-]+\/artwork$/,
      /^404 \/api\/v1\/playlists\/[0-9a-f-]+\/download$/,
      /^409 \/api\/v1\/sources\/[0-9a-f-]+\/audio$/,
    ];
    expect(
      failed.filter(
        (entry) => !expected.some((pattern) => pattern.test(entry)),
      ),
    ).toEqual([]);
  } finally {
    await automation(true);
    if (connection)
      await page.request.delete(`/api/v1/plugins/${connection.id}`, {
        headers,
        data: { revision: connection.revision },
      });
    const copies: { id: string; name: string }[] = await (
      await page.request.get("/api/v1/playlists")
    ).json();
    for (const copy of copies.filter((item) =>
      item.name.startsWith(fixture.prefix),
    ))
      await page.request.post("/api/v1/sync", {
        headers,
        data: {
          device_id: "featured-e2e-cleanup",
          operations: [
            {
              id: randomUUID(),
              action: { kind: "playlist_delete", playlist_id: copy.id },
            },
          ],
        },
      });
    if (service)
      await page.request.delete(`/api/v1/plugins/services/${service.id}`, {
        headers,
        data: { revision: service.revision },
      });
    await fixture.close();
  }
});
