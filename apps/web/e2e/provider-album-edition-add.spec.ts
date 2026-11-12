import { randomUUID } from "node:crypto";
import { expect, test } from "@playwright/test";
import type { Schema } from "../src/shared/schema";
import {
  albumEdition,
  albumEditionProvider,
} from "./fixtures/album-edition-provider";

const owned = /Ready|In your library/;

test("the add button on a catalog-only edition row saves exactly that track without playing it", async ({
  page,
  baseURL,
}) => {
  const label = randomUUID().slice(0, 8);
  const fixture = await albumEditionProvider(label);
  const headers = { "x-hito-request": "1", origin: new URL(baseURL!).origin };
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("console", (message) => {
    if (
      message.type() === "error" &&
      !message.text().startsWith("Failed to load resource")
    )
      errors.push(message.text());
  });
  const audioStreams: string[] = [];
  page.on("request", (request) => {
    if (/\/api\/v1\/sources\/[^/]+\/audio/.test(request.url()))
      audioStreams.push(request.url());
  });
  let connection: Schema<"PluginInstallation"> | null = null;
  let original: Schema<"MetadataSettings"> | null = null;
  const post = async <T>(path: string, data: unknown, status = 200) => {
    const response = await page.request.post(path, { headers, data });
    expect(response.status(), path).toBe(status);
    return (await response.json()) as T;
  };
  try {
    const service = await post<Schema<"PluginService">>(
      "/api/v1/plugins/services",
      {
        name: "Album edition add collection",
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

    const current = await page.request.get("/api/v1/metadata/settings");
    original = await current.json();
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

    await page.goto(`/albums/${release}`);
    await page
      .getByRole("button", { name: "Show complete track list" })
      .click();
    await page.getByRole("radio", { name: /Deluxe/ }).check();
    await page.getByRole("button", { name: "Use this" }).click();

    const region = page.getByRole("region", {
      name: "Album edition track list",
    });
    const rows = region.getByRole("listitem");
    await expect(rows).toHaveCount(4);
    const bonus = rows.nth(2);
    await expect(bonus).toContainText("Catalog bonus");
    await expect(bonus).toContainText("Not in your library");
    await expect(
      bonus.getByRole("img", { name: "In your library" }),
    ).toHaveCount(0);

    const title = fixture.titles[albumEdition.bonus];
    const before = await page.request.get(
      `/api/v1/recordings?q=${encodeURIComponent(title)}`,
    );
    expect((await before.json()).total).toBe(0);

    const dock = page.getByRole("contentinfo", {
      name: "Audio player",
      exact: true,
    });
    const playButton = dock.getByRole("button", { name: /^(Play|Pause)$/ });
    const queueButton = dock.getByRole("button", { name: "Open queue" });
    const playLabel = await playButton.getAttribute("aria-label");
    const queueText = await queueButton.innerText();
    const dockTitle = await dock.locator("strong").first().innerText();
    expect(playLabel).toBe("Play");
    const callsBefore = fixture.calls.length;
    const detailsBefore = fixture.details.length;

    await bonus
      .getByRole("button", { name: "Add Catalog bonus to your library" })
      .click();

    await expect(bonus).toContainText(owned);
    await expect(bonus).not.toContainText("Not in your library");
    await expect(
      bonus.getByRole("img", { name: "In your library" }),
    ).toBeVisible();
    await expect(
      bonus.getByRole("button", { name: /^Add .* to your library$/ }),
    ).toHaveCount(0);
    await expect(
      rows.nth(0).getByRole("button", { name: /^Add .* to your library$/ }),
    ).toHaveCount(0);

    expect(fixture.details.slice(detailsBefore)).toEqual([albumEdition.bonus]);
    const newCalls = fixture.calls.slice(callsBefore);
    expect(
      newCalls.filter((call) => call === "v1/recordings/detail"),
    ).toHaveLength(1);
    expect(newCalls.filter((call) => call === "v1/sources")).toHaveLength(1);
    expect(
      newCalls.filter(
        (call) =>
          call !== "v1/recordings/detail" &&
          call !== "v1/sources" &&
          call !== "v1/catalog/editions/track-list",
      ),
    ).toEqual([]);

    const adopted = await page.request.get(
      `/api/v1/recordings?q=${encodeURIComponent(title)}`,
    );
    const adoptedBody = await adopted.json();
    expect(adoptedBody.total).toBe(1);
    expect(JSON.stringify(adoptedBody)).toContain(title);

    await page.waitForTimeout(500);
    expect(audioStreams).toEqual([]);
    expect(fixture.audioRequests).toEqual([]);
    await expect
      .poll(() =>
        page
          .locator("audio[data-hito-player]")
          .evaluateAll((elements) =>
            elements.every((element) => {
              const audio = element as HTMLAudioElement;
              return audio.paused && audio.currentTime === 0;
            }),
          ),
      )
      .toBe(true);
    await expect(playButton).toHaveAttribute("aria-label", playLabel!);
    await expect(queueButton).toHaveText(queueText);
    await expect(dock.locator("strong").first()).toHaveText(dockTitle);
    expect(fixture.state.unexpected).toBe(0);
    expect(errors).toEqual([]);
  } finally {
    if (original && connection) {
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
    await fixture.close();
  }
});
