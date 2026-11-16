import { randomUUID } from "node:crypto";
import { expect, test } from "@playwright/test";
import type { Schema } from "../src/shared/schema";
import { openPlaylistImport } from "./fixtures/playlist-dialogs";
import { playlistFixture } from "./fixtures/playlist";
import { providerFixture } from "./fixtures/provider";
import { shoot } from "./fixtures/ux-shots";

test("a pasted link finds a source that the listener reviews before it is added", async ({
  page,
  baseURL,
}) => {
  if (!baseURL)
    throw new Error("The disposable e2e server URL must be configured");
  const playlist = await playlistFixture();
  const provider = await providerFixture("Link", false, false, false, true);
  const headers = { "x-hito-request": "1", origin: new URL(baseURL).origin };
  const services: Schema<"PluginService">[] = [];
  const connections: Schema<"PluginInstallation">[] = [];
  const errors: string[] = [];
  let copied: string | null = null;
  page.on("pageerror", (error) => errors.push(error.message));
  try {
    await page.goto("/library");
    for (const [index, fixture] of [playlist, provider].entries()) {
      const offered = await page.request.post("/api/v1/plugins/services", {
        headers,
        data: {
          name: `${playlist.prefix} link source ${index}`,
          base_url: fixture.address,
        } satisfies Schema<"OfferPluginService">,
      });
      expect(offered.status()).toBe(201);
      const service: Schema<"PluginService"> = await offered.json();
      services.push(service);
      const installed = await page.request.post("/api/v1/plugins", {
        headers,
        data: {
          service_id: service.id,
          service_revision: service.revision,
          bearer_token: fixture.token,
        } satisfies Schema<"InstallPlugin">,
      });
      expect(installed.status()).toBe(201);
      connections.push(await installed.json());
    }
    await page.goto("/playlists");
    await openPlaylistImport(page);
    const form = page.getByRole("region", { name: "Import a playlist" });
    await form
      .getByRole("textbox", { name: "Playlist link or identifier" })
      .fill(playlist.reference);
    const imported = page.waitForResponse(
      (response) =>
        new URL(response.url()).pathname ===
          `/api/v1/plugins/${connections[0].id}/playlists/import` &&
        response.request().method() === "POST",
    );
    await form
      .getByRole("button", { name: "Import a copy", exact: true })
      .click();
    const receipt: Schema<"PlaylistImportReceipt"> = await (
      await imported
    ).json();
    copied = receipt.playlist_id;
    const detail = await (
      await page.request.get(`/api/v1/playlists/${receipt.playlist_id}`)
    ).json();
    const recording = (detail as Schema<"PlaylistDetail">).entries[0]
      .recording_id;
    if (!recording) throw new Error("The copied track needs a saved recording");

    await page.goto(`/recordings/${recording}`);
    await page
      .getByRole("button", { name: "Find audio", exact: true })
      .first()
      .click();
    const link = page.getByRole("textbox", { name: "Paste a link" });
    await expect(link).toBeVisible();
    await shoot(page, "source-link-field", { focus: link });
    await link.fill("https://owned-links.invalid/track/1");
    const looked = page.waitForResponse(
      (response) =>
        new URL(response.url()).pathname ===
        `/api/v1/recordings/${recording}/sources/lookup`,
    );
    await page.getByRole("button", { name: "Look up link" }).click();
    expect((await looked).status()).toBe(200);
    const review = page.getByRole("dialog", { name: "Review source match" });
    await expect(review).toBeVisible();
    await expect(review.getByText(provider.recording.title)).toBeVisible();
    await shoot(page, "source-link-review");
    await review.getByRole("button", { name: "Use this source" }).click();
    await expect(page.getByText("Source added to this track.")).toBeVisible();
    expect(provider.lookups).toEqual(["https://owned-links.invalid/track/1"]);
    const sources: Schema<"Source">[] = await (
      await page.request.get(`/api/v1/recordings/${recording}/sources`)
    ).json();
    expect(sources.length).toBeGreaterThan(0);
    expect(errors).toEqual([]);
  } finally {
    for (const connection of connections)
      await page.request.delete(`/api/v1/plugins/${connection.id}`, {
        headers,
        data: { revision: connection.revision },
      });
    if (copied)
      await page.request.post("/api/v1/sync", {
        headers,
        data: {
          device_id: "source-link-e2e-cleanup",
          operations: [
            {
              id: randomUUID(),
              action: { kind: "playlist_delete", playlist_id: copied },
            },
          ],
        } satisfies Schema<"SyncBatch">,
      });
    for (const service of services)
      await page.request.delete(`/api/v1/plugins/services/${service.id}`, {
        headers,
        data: { revision: service.revision },
      });
    await Promise.all([playlist.close(), provider.close()]);
  }
});
