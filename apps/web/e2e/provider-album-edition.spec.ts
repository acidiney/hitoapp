import { randomUUID } from "node:crypto";
import { expect, test, type Page } from "@playwright/test";
import type { Schema } from "../src/shared/schema";
import {
  albumEdition,
  albumEditionProvider,
} from "./fixtures/album-edition-provider";

const owned = /Ready|In your library/;

async function openEditionMenu(page: Page, item: string) {
  await page.getByRole("button", { name: /^Edition / }).click();
  await page.getByRole("menuitem", { name: item }).click();
}

test("an album compares a chosen edition with its library and saves a catalog track only when played", async ({
  page,
  baseURL,
}) => {
  const label = randomUUID().slice(0, 8);
  const fixture = await albumEditionProvider(label);
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
        name: "Album edition collection",
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
    expect(saved.every((item) => item.release_id === release)).toBe(true);

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

    const detailsBefore = fixture.details.length;
    await page.goto(`/albums/${release}`);
    await expect(
      page.getByRole("button", { name: "Choose an edition" }),
    ).toBeVisible();
    expect(fixture.calls.filter((call) => call.includes("editions"))).toEqual(
      [],
    );
    const question = page.getByRole("dialog", {
      name: `Is this ${albumEdition.releaseTitle}?`,
    });
    await page.getByRole("button", { name: "Choose an edition" }).click();
    await expect(question).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(question).toHaveCount(0);
    await page.getByRole("button", { name: "Choose an edition" }).click();
    await expect(question).toBeVisible();
    await expect(
      page.getByRole("list", { name: "Discography releases" }),
    ).toHaveCount(0);
    await question.getByRole("radio", { name: /Deluxe/ }).check();
    await question.getByRole("button", { name: "Use this" }).click();
    await expect(question).toHaveCount(0);

    const region = page.getByRole("region", {
      name: "Album edition track list",
    });
    await expect(
      region.getByRole("button", { name: /^Edition / }),
    ).toBeVisible();
    const rows = region.getByRole("listitem");
    await expect(rows).toHaveCount(4);
    await expect(rows.nth(0)).toContainText(owned);
    await expect(rows.nth(1)).toContainText(owned);
    await expect(rows.nth(2)).toContainText("Not in your library");
    await expect(rows.nth(2)).toContainText("Catalog bonus");
    await expect(rows.nth(3)).toContainText(owned);
    await expect(rows.nth(3)).toContainText("Catalog one again");
    expect(fixture.details.length).toBe(detailsBefore);
    expect(fixture.audioRequests).toEqual([]);
    const library = await page.request.get(
      `/api/v1/recordings?q=${encodeURIComponent(fixture.titles[albumEdition.bonus])}`,
    );
    expect((await library.json()).total).toBe(0);

    await rows
      .nth(2)
      .getByRole("button", { name: /^Play / })
      .click();
    await expect(rows.nth(2)).toContainText(owned);
    const audio = page.locator("audio[data-hito-player]");
    await expect
      .poll(() =>
        audio.evaluate((element: HTMLAudioElement) => element.currentTime),
      )
      .toBeGreaterThan(0.2);
    expect(fixture.details.slice(detailsBefore)).toEqual([albumEdition.bonus]);
    expect(fixture.audioRequests.length).toBeGreaterThan(0);
    const adopted = await page.request.get(
      `/api/v1/recordings?q=${encodeURIComponent(fixture.titles[albumEdition.bonus])}`,
    );
    expect((await adopted.json()).total).toBe(1);

    fixture.state.outage = true;
    await openEditionMenu(page, "Change edition");
    const change = page.getByRole("dialog", {
      name: albumEdition.releaseTitle,
      exact: true,
    });
    await expect(change).toBeVisible();
    await change.getByRole("radio", { name: /Standard/ }).check();
    await change.getByRole("button", { name: "Use this" }).click();
    await expect(change).toHaveCount(0);
    await expect(page.getByText(/track list is unavailable/)).toBeVisible();
    await expect(
      page.getByRole("table", { name: "Tracks" }).getByRole("row"),
    ).not.toHaveCount(0);
    fixture.state.outage = false;
    await expect(
      page.getByRole("heading", { name: "Showing only your tracks" }),
    ).toBeVisible();
    await page
      .getByRole("region", { name: "Album edition track list" })
      .getByRole("button", { name: "Try again", exact: true })
      .click();
    await expect(region.getByRole("listitem")).toHaveCount(3);
    const lastDisc = region
      .getByRole("list", { name: /^Disc \d+ tracks$/ })
      .last();
    await expect(lastDisc).toContainText(fixture.titles[albumEdition.second]);
    await expect(lastDisc).toContainText(fixture.titles[albumEdition.bonus]);

    await expect
      .poll(async () => {
        const detail = await page.request.get(`/api/v1/releases/${release}`);
        return (await detail.json()).saved_edition?.state?.edition?.edition_id;
      })
      .toBe(albumEdition.standardId);
    const catalogCalls = fixture.calls.length;
    await page.reload();
    await expect(
      page.getByRole("button", { name: "Choose an edition" }),
    ).toHaveCount(0);
    const remembered = page.getByRole("region", {
      name: "Album edition track list",
    });
    await expect(remembered.getByRole("listitem")).toHaveCount(3);
    await expect(
      remembered.getByRole("button", { name: /^Edition / }),
    ).toBeVisible();
    const reread = fixture.calls.slice(catalogCalls);
    expect(reread.filter((call) => call.endsWith("track-list"))).toHaveLength(
      1,
    );
    expect(
      reread.filter((call) => /discography|profile|release-groups/.test(call)),
    ).toEqual([]);

    await openEditionMenu(page, "Forget edition");
    await expect(
      page.getByRole("button", { name: "Choose an edition" }),
    ).toBeVisible();
    await page.reload();
    await expect(
      page.getByRole("button", { name: "Choose an edition" }),
    ).toBeVisible();
    const cleared = await page.request.get(`/api/v1/releases/${release}`);
    expect((await cleared.json()).saved_edition.state).toEqual({
      status: "unset",
    });
    expect(fixture.state.unexpected).toBe(0);
    expect(errors).toEqual([]);
    const privateValues = [fixture.token, label, albumEdition.groupId];
    for (const value of privateValues)
      expect(messages.filter((message) => message.includes(value))).toEqual([]);
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
