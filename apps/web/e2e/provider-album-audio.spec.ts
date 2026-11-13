import { randomUUID } from "node:crypto";
import { expect, test, type Page } from "@playwright/test";
import type { Schema } from "../src/shared/schema";
import {
  albumEdition,
  albumEditionProvider,
  unidentified,
} from "./fixtures/album-edition-provider";
import { albumAudioProvider } from "./fixtures/album-audio-provider";

async function openAlbum(page: Page, baseURL: string) {
  const label = randomUUID().slice(0, 8);
  const catalog = await albumEditionProvider(label, true);
  const audio = await albumAudioProvider();
  const headers = { "x-hito-request": "1", origin: new URL(baseURL).origin };
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  const messages: string[] = [];
  page.on("console", (message) => {
    messages.push(message.text());
    if (
      message.type() === "error" &&
      !message.text().startsWith("Failed to load resource")
    )
      errors.push(message.text());
  });
  const connections: Schema<"PluginInstallation">[] = [];
  let original: Schema<"MetadataSettings"> | null = null;
  const post = async <T>(path: string, data: unknown, status = 200) => {
    const response = await page.request.post(path, { headers, data });
    expect(response.status(), path).toBe(status);
    return (await response.json()) as T;
  };
  const connect = async (name: string, address: string, token: string) => {
    const service = await post<Schema<"PluginService">>(
      "/api/v1/plugins/services",
      { name, base_url: address } satisfies Schema<"OfferPluginService">,
      201,
    );
    const connection = await post<Schema<"PluginInstallation">>(
      "/api/v1/plugins",
      {
        service_id: service.id,
        service_revision: service.revision,
        bearer_token: token,
      } satisfies Schema<"InstallPlugin">,
      201,
    );
    connections.push(connection);
    return connection;
  };
  const dispose = async () => {
    if (original && connections.length > 0) {
      const latest = await (
        await page.request.get("/api/v1/metadata/settings")
      ).json();
      const restored = await page.request.put("/api/v1/metadata/settings", {
        headers,
        data: {
          revision: latest.revision,
          enabled: original.enabled,
          providers: original.providers,
        } satisfies Schema<"UpdateMetadataSettings">,
      });
      expect(restored.status()).toBe(200);
    }
    for (const connection of connections.reverse()) {
      const removed = await page.request.delete(
        `/api/v1/plugins/${connection.id}`,
        { headers, data: { revision: connection.revision } },
      );
      expect(removed.status(), "plugin cleanup").toBeLessThan(300);
    }
    await catalog.close();
    await audio.close();
  };
  try {
    const connection = await connect(
      "Album catalog fixture",
      catalog.address,
      catalog.token,
    );
    const saved: Schema<"Recording">[] = [];
    for (const id of [albumEdition.first, albumEdition.second]) {
      saved.push(
        await post<Schema<"Recording">>(
          `/api/v1/plugins/${connection.id}/recordings`,
          {
            installation_revision: connection.revision,
            provider_recording_id: id,
          } satisfies Schema<"AdoptProviderRecording">,
        ),
      );
    }
    const release = saved[0].release_id!;
    original = await (
      await page.request.get("/api/v1/metadata/settings")
    ).json();
    const provider = {
      kind: "http_plugin",
      installation_id: connection.id,
    } satisfies Schema<"MetadataProvider">;
    const updated = await page.request.put("/api/v1/metadata/settings", {
      headers,
      data: {
        revision: original!.revision,
        enabled: true,
        providers: [provider],
      } satisfies Schema<"UpdateMetadataSettings">,
    });
    expect(updated.status()).toBe(200);
    const review = `/api/v1/recordings/${saved[0].id}/artist-review`;
    const context = await post<Schema<"ArtistReviewContext">>(
      `${review}/context`,
      { provider } satisfies Schema<"BeginArtistReview">,
    );
    const choice = {
      context,
      provider_artist_id: albumEdition.artistId,
    } satisfies Schema<"SelectArtistCandidate">;
    await post(`${review}/preview`, choice);
    await post(`${review}/confirm`, choice);
    const audioConnection = await connect(
      "Album audio fixture",
      audio.address,
      audio.token,
    );
    expect(audioConnection.id).not.toBe(connection.id);

    await page.goto(`/albums/${release}`);
    await page.getByRole("button", { name: "Choose an edition" }).click();
    await page.getByRole("radio", { name: /Deluxe/ }).check();
    await page.getByRole("button", { name: "Use this" }).click();
    const region = page.getByRole("region", {
      name: "Album edition track list",
    });
    const row = region.getByRole("listitem").filter({ hasText: unidentified });
    await expect(row).toContainText("No provider identity for this track");
    await expect(row).toContainText("Not in your library");
    const total = async () =>
      (await (await page.request.get("/api/v1/recordings")).json()).total;
    return {
      catalog,
      audio,
      label,
      messages,
      errors,
      region,
      row,
      saved,
      total,
      before: await total(),
      find: row.getByRole("button", {
        name: `Add ${unidentified} to your library`,
      }),
      play: row.getByRole("button", {
        name: `Play ${unidentified}`,
        exact: true,
      }),
      dispose,
    };
  } catch (error) {
    await dispose();
    throw error;
  }
}

async function expectClean(album: Awaited<ReturnType<typeof openAlbum>>) {
  expect(album.catalog.state.unexpected).toBe(0);
  expect(album.audio.state.unexpected).toBe(0);
  expect(album.errors).toEqual([]);
  for (const value of [album.catalog.token, album.audio.token, album.label])
    expect(album.messages.filter((message) => message.includes(value))).toEqual(
      [],
    );
}

test("an album track without a catalog identity is saved as a best guess when only a different title is ready", async ({
  page,
  baseURL,
}) => {
  const album = await openAlbum(page, baseURL!);
  try {
    const { audio, row, find, before, total } = album;
    audio.state.offer = {
      title: "A different song",
      artist: albumEdition.artistName,
      duration_ms: 1000,
    };
    await find.click();
    await expect(
      row.getByRole("img", { name: "In your library" }),
    ).toBeVisible();
    expect(await total()).toBe(before + 1);
    expect(audio.audioRequests).toEqual([]);
    await album.play.click();
    const player = page.locator("audio[data-hito-player]");
    await expect
      .poll(() =>
        player.evaluate((element: HTMLAudioElement) => element.currentTime),
      )
      .toBeGreaterThan(0.2);
    expect(audio.audioRequests.length).toBeGreaterThan(0);

    const adopted = await page.request.get(
      `/api/v1/recordings?q=${encodeURIComponent("A different song")}`,
    );
    const found = (await adopted.json()).items[0] as Schema<"Recording">;
    expect(found.title).toBe("A different song");
    expect(found.id).not.toBe(album.saved[0].id);
    const sources = (await (
      await page.request.get(`/api/v1/recordings/${found.id}/sources`)
    ).json()) as Schema<"Source">[];
    expect(sources[0]).toMatchObject({ match_kind: "best_guess" });

    await page.goto(`/recordings/${found.id}?tab=sources`);
    const panel = page.getByRole("region", { name: "Playback source" });
    await expect(panel.getByText("Best guess")).toBeVisible();
    await expect(
      panel.getByRole("button", { name: "Fix source" }),
    ).toBeVisible();

    audio.state.offer = {
      title: unidentified,
      artist: albumEdition.artistName,
      duration_ms: 1000,
    };
    const requests = audio.audioRequests.length;
    const again = (await (
      await page.request.get(`/api/v1/recordings/${found.id}/sources`)
    ).json()) as Schema<"Source">[];
    expect(again[0]).toMatchObject({ match_kind: "best_guess" });
    expect(await total()).toBe(before + 1);
    expect(audio.audioRequests.length).toBe(requests);
    await expectClean(album);
  } finally {
    await album.dispose();
  }
});

test("an album track whose provider has no valid ready source finds no match and publishes nothing", async ({
  page,
  baseURL,
}) => {
  const album = await openAlbum(page, baseURL!);
  try {
    const { audio, row, find, before, total } = album;
    audio.state.ready = false;
    audio.state.offer = {
      title: "A different song",
      artist: albumEdition.artistName,
      duration_ms: 1000,
    };
    await find.click();
    await expect(row.getByRole("alert")).toContainText("confident match");
    await expect(row).toContainText("Not in your library");
    expect(await total()).toBe(before);
    expect(audio.audioRequests).toEqual([]);

    audio.state.offer = {
      title: unidentified,
      artist: albumEdition.artistName,
      duration_ms: 1000,
    };
    await find.click();
    await expect(row.getByRole("alert")).toContainText("confident match");
    expect(await total()).toBe(before);
    expect(audio.audioRequests).toEqual([]);

    audio.state.ready = true;
    await find.click();
    await expect(
      row.getByRole("img", { name: "In your library" }),
    ).toBeVisible();
    expect(await total()).toBe(before + 1);
    const adopted = await page.request.get(
      `/api/v1/recordings?q=${encodeURIComponent(unidentified)}`,
    );
    const found = (await adopted.json()).items[0] as Schema<"Recording">;
    expect(found.title).toBe(unidentified);
    const sources = (await (
      await page.request.get(`/api/v1/recordings/${found.id}/sources`)
    ).json()) as Schema<"Source">[];
    expect(sources[0]).toMatchObject({ match_kind: "verified" });
    await expectClean(album);
  } finally {
    await album.dispose();
  }
});

test("an album track audio lookup during a provider outage is unavailable, not a no-match, and recovers", async ({
  page,
  baseURL,
}) => {
  const album = await openAlbum(page, baseURL!);
  try {
    const { audio, region, row, find, before, total } = album;
    audio.state.outage = true;
    await find.click();
    await expect(row.getByRole("alert")).toContainText("unavailable");
    await expect(row.getByRole("alert")).not.toContainText("confident match");
    await expect(
      region.getByRole("button", { name: /^Play Album edition one/ }).first(),
    ).toBeVisible();
    expect(await total()).toBe(before);

    audio.state.outage = false;
    audio.state.offer = {
      title: unidentified,
      artist: albumEdition.artistName,
      duration_ms: 1000,
    };
    await find.click();
    await expect(
      row.getByRole("img", { name: "In your library" }),
    ).toBeVisible();
    await album.play.click();
    const player = page.locator("audio[data-hito-player]");
    await expect
      .poll(() =>
        player.evaluate((element: HTMLAudioElement) => element.currentTime),
      )
      .toBeGreaterThan(0.2);
    expect(audio.audioRequests.length).toBeGreaterThan(0);
    expect(await total()).toBe(before + 1);
    const adopted = await page.request.get(
      `/api/v1/recordings?q=${encodeURIComponent(unidentified)}`,
    );
    const found = (await adopted.json()).items[0] as Schema<"Recording">;
    expect(found.title).toBe(unidentified);
    expect(found.id).not.toBe(album.saved[0].id);
    await expectClean(album);
  } finally {
    await album.dispose();
  }
});
