import { randomUUID } from "node:crypto";
import { expect, test } from "@playwright/test";
import type { components } from "../src/generated";
import {
  albumEdition,
  albumEditionProvider,
  unidentified,
} from "./fixtures/album-edition-provider";
import { albumAudioProvider } from "./fixtures/album-audio-provider";

type Schema = components["schemas"];

test("an album track without a catalog identity finds approved audio, rejects uncertain matches and survives outages", async ({
  page,
  baseURL,
}) => {
  const label = randomUUID().slice(0, 8);
  const catalog = await albumEditionProvider(label, true);
  const audio = await albumAudioProvider();
  const headers = { "x-hito-request": "1", origin: new URL(baseURL!).origin };
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
  const connections: Schema["PluginInstallation"][] = [];
  let original: Schema["MetadataSettings"] | null = null;
  const post = async <T>(path: string, data: unknown, status = 200) => {
    const response = await page.request.post(path, { headers, data });
    expect(response.status(), path).toBe(status);
    return (await response.json()) as T;
  };
  const connect = async (name: string, address: string, token: string) => {
    const service = await post<Schema["PluginService"]>(
      "/api/v1/plugins/services",
      { name, base_url: address } satisfies Schema["OfferPluginService"],
      201,
    );
    const connection = await post<Schema["PluginInstallation"]>(
      "/api/v1/plugins",
      {
        service_id: service.id,
        service_revision: service.revision,
        bearer_token: token,
      } satisfies Schema["InstallPlugin"],
      201,
    );
    connections.push(connection);
    return connection;
  };
  const total = async () =>
    (await (await page.request.get("/api/v1/recordings")).json()).total;
  try {
    const connection = await connect(
      "Album catalog fixture",
      catalog.address,
      catalog.token,
    );
    const saved: Schema["Recording"][] = [];
    for (const id of [albumEdition.first, albumEdition.second]) {
      saved.push(
        await post<Schema["Recording"]>(
          `/api/v1/plugins/${connection.id}/recordings`,
          {
            installation_revision: connection.revision,
            provider_recording_id: id,
          } satisfies Schema["AdoptProviderRecording"],
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
    } satisfies Schema["MetadataProvider"];
    const updated = await page.request.put("/api/v1/metadata/settings", {
      headers,
      data: {
        revision: original!.revision,
        enabled: true,
        providers: [provider],
      } satisfies Schema["UpdateMetadataSettings"],
    });
    expect(updated.status()).toBe(200);
    const review = `/api/v1/recordings/${saved[0].id}/artist-review`;
    const context = await post<Schema["ArtistReviewContext"]>(
      `${review}/context`,
      { provider } satisfies Schema["BeginArtistReview"],
    );
    const choice = {
      context,
      provider_artist_id: albumEdition.artistId,
    } satisfies Schema["SelectArtistCandidate"];
    await post(`${review}/preview`, choice);
    await post(`${review}/confirm`, choice);
    const audioConnection = await connect(
      "Album audio fixture",
      audio.address,
      audio.token,
    );

    await page.goto(`/albums/${release}`);
    await page
      .getByRole("button", { name: "Show complete track list" })
      .click();
    const groups = page.getByRole("list", { name: "Discography releases" });
    await groups.getByRole("button", { name: "Browse editions" }).click();
    const editions = page.getByRole("list", { name: "Available editions" });
    await editions
      .getByRole("listitem")
      .filter({ hasText: "Deluxe" })
      .getByRole("button", { name: "Choose edition" })
      .click();
    const region = page.getByRole("region", {
      name: "Album edition track list",
    });
    const row = region.getByRole("listitem").filter({ hasText: unidentified });
    await expect(row).toContainText("No provider identity for this track");
    await expect(row).toContainText("Not in your library");
    const before = await total();
    const find = row.getByRole("button", {
      name: `Find audio for ${unidentified}`,
    });

    audio.state.outage = true;
    await find.click();
    await expect(row.getByRole("alert")).toContainText("unavailable");
    await expect(
      region.getByRole("button", { name: /^Play Album edition one/ }).first(),
    ).toBeVisible();
    expect(await total()).toBe(before);

    audio.state.outage = false;
    audio.state.offer = {
      title: "A different song",
      artist: albumEdition.artistName,
      duration_ms: 1000,
    };
    await find.click();
    await expect(row.getByRole("alert")).toContainText("confident match");
    expect(await total()).toBe(before);
    expect(audio.audioRequests).toEqual([]);

    audio.state.offer = {
      title: unidentified,
      artist: albumEdition.artistName,
      duration_ms: 1000,
    };
    await find.click();
    await expect(row).toContainText("In your library");
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
    const found = (await adopted.json()).items[0] as Schema["Recording"];
    expect(found.title).toBe(unidentified);
    expect(found.id).not.toBe(saved[0].id);
    expect(catalog.state.unexpected).toBe(0);
    expect(audio.state.unexpected).toBe(0);
    expect(errors).toEqual([]);
    for (const value of [catalog.token, audio.token, label])
      expect(messages.filter((message) => message.includes(value))).toEqual([]);
    expect(audioConnection.id).not.toBe(connection.id);
  } finally {
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
        } satisfies Schema["UpdateMetadataSettings"],
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
  }
});
