import { randomUUID } from "node:crypto";
import { expect, test, type Page } from "@playwright/test";
import type { Schema } from "../src/shared/schema";
import {
  albumEdition,
  albumEditionProvider,
} from "./fixtures/album-edition-provider";
import { createMembers, memberPassword } from "./fixtures/member-accounts";

const longTrack = 900_000;

async function savedAlbum(
  page: Page,
  baseURL: string,
  durations: (number | null)[],
) {
  const fixture = await albumEditionProvider(randomUUID().slice(0, 8));
  fixture.state.durations = durations;
  const headers = { "x-hito-request": "1", origin: new URL(baseURL).origin };
  let connection: Schema<"PluginInstallation"> | null = null;
  let original: Schema<"MetadataSettings"> | null = null;
  const post = async <T>(path: string, data: unknown, status = 200) => {
    const response = await page.request.post(path, { headers, data });
    expect(response.status(), path).toBe(status);
    return (await response.json()) as T;
  };
  const close = async () => {
    try {
      if (original) {
        const latest = await (
          await page.request.get("/api/v1/metadata/settings")
        ).json();
        await page.request.put("/api/v1/metadata/settings", {
          headers,
          data: {
            revision: latest.revision,
            enabled: original.enabled,
            providers: original.providers,
          } satisfies Schema<"UpdateMetadataSettings">,
        });
      }
      if (connection)
        await page.request.delete(`/api/v1/plugins/${connection.id}`, {
          headers,
          data: { revision: connection.revision },
        });
    } finally {
      await fixture.close();
    }
  };
  try {
    const service = await post<Schema<"PluginService">>(
      "/api/v1/plugins/services",
      {
        name: "Saved album stats fixture",
        base_url: fixture.address,
      } satisfies Schema<"OfferPluginService">,
      201,
    );
    connection = await post<Schema<"PluginInstallation">>(
      "/api/v1/plugins",
      {
        service_id: service.id,
        service_revision: service.revision,
        bearer_token: fixture.token,
      } satisfies Schema<"InstallPlugin">,
      201,
    );
    const saved: Schema<"Recording">[] = [];
    for (const id of [albumEdition.first, albumEdition.second])
      saved.push(
        await post<Schema<"Recording">>(
          `/api/v1/plugins/${connection.id}/recordings`,
          {
            installation_revision: connection.revision,
            provider_recording_id: id,
          } satisfies Schema<"AdoptProviderRecording">,
        ),
      );
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
    const release = saved[0].release_id!;
    await page.goto(`/albums/${release}`);
    await page.getByRole("button", { name: "Choose an edition" }).click();
    await page.getByRole("radio", { name: /Deluxe/ }).check();
    await page.getByRole("button", { name: "Use this" }).click();
    const region = page.getByRole("region", {
      name: "Album edition track list",
    });
    await expect(region.getByRole("listitem")).toHaveCount(4);
    return { fixture, release, region, close };
  } catch (error) {
    await close();
    throw error;
  }
}

const stats = (page: Page) => page.locator(".album-header .album-stats");

test("a saved album edition with one unknown track length shows its track count without a length", async ({
  page,
  baseURL,
}) => {
  test.setTimeout(120_000);
  const album = await savedAlbum(page, baseURL!, [
    longTrack,
    null,
    longTrack,
    longTrack,
  ]);
  try {
    await expect(stats(page)).toHaveText(/ · 4 tracks$/);
    await album.region
      .getByRole("listitem")
      .nth(0)
      .getByRole("button", { name: /^Play / })
      .click();
    const player = page.locator("audio[data-hito-player]");
    await expect
      .poll(() =>
        player.evaluate((element: HTMLAudioElement) => element.currentTime),
      )
      .toBeGreaterThan(0.2);
    await expect(stats(page)).toHaveText(/ · 4 tracks$/);
    expect(album.fixture.state.unexpected).toBe(0);
  } finally {
    await album.close();
  }
});

test("a saved album edition with known track lengths shows the total the server reports", async ({
  page,
  baseURL,
}) => {
  test.setTimeout(120_000);
  const album = await savedAlbum(page, baseURL!, [
    longTrack,
    longTrack,
    longTrack,
    longTrack,
  ]);
  try {
    await expect(stats(page)).toHaveText(/ · 4 tracks · 1 hr 0 min$/);
    expect(album.fixture.state.unexpected).toBe(0);
  } finally {
    await album.close();
  }
});

test("another account does not see someone else's chosen edition or its totals", async ({
  page,
  browser,
  baseURL,
}) => {
  test.setTimeout(120_000);
  const album = await savedAlbum(page, baseURL!, [
    longTrack,
    longTrack,
    longTrack,
    longTrack,
  ]);
  let members: Awaited<ReturnType<typeof createMembers>> | undefined;
  let peer: Awaited<ReturnType<typeof browser.newContext>> | undefined;
  try {
    await expect(stats(page)).toHaveText(/ · 4 tracks · 1 hr 0 min$/);
    members = await createMembers(browser, baseURL!, "saved-stats", 1);
    peer = await browser.newContext({ baseURL });
    const login = await peer.request.post("/api/v1/login", {
      headers: { "x-hito-request": "1", origin: new URL(baseURL!).origin },
      data: { username: members.names[0], password: memberPassword },
    });
    expect(login.status()).toBe(200);
    const other = await peer.newPage();
    const response = await other.goto(`/albums/${album.release}`);
    expect([200, 404]).toContain(response?.status());
    await expect(other.getByText(/1 hr 0 min/)).toHaveCount(0);
    await expect(
      other.getByRole("region", { name: "Album edition track list" }),
    ).toHaveCount(0);
  } finally {
    try {
      await peer?.close();
    } finally {
      try {
        await members?.close();
      } finally {
        await album.close();
      }
    }
  }
});
