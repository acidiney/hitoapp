import { expect, test } from "@playwright/test";
import type { Schema } from "../src/shared/schema";
import { catalogAlbum } from "./fixtures/catalog-album-provider";
import { connectCatalogArtist } from "./fixtures/catalog-album-setup";

const [opening, interlude, hidden, closing] = catalogAlbum.tracks;

test("a catalog album plays like a library album: Play starts the first track, queues what services have and skips what they lack", async ({
  page,
  baseURL,
}) => {
  test.setTimeout(120_000);
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  const artist = await connectCatalogArtist(page, baseURL!);
  artist.provider.state.available.delete(hidden);
  try {
    await page.goto(artist.path);
    await page
      .getByRole("link", { name: `Open ${catalogAlbum.groupTitle}` })
      .click();
    await expect(page).toHaveURL(/\/releases\//);
    await expect(
      page.getByRole("heading", { name: catalogAlbum.groupTitle }),
    ).toHaveCount(1);
    await expect(
      page.getByRole("button", { name: "Choose edition" }),
    ).toHaveCount(0);
    await expect(page.getByText(/Audio availability has not been/)).toHaveCount(
      0,
    );
    const table = page.getByRole("list", { name: "Tracks" }).first();
    await expect(table.getByRole("listitem")).toHaveCount(4);
    expect(artist.provider.state.searches).toEqual([]);

    await page.getByRole("button", { name: "Play album" }).click();
    const audio = page.locator("audio[data-hito-player]");
    await expect
      .poll(() =>
        audio.evaluate((element: HTMLAudioElement) => element.currentTime),
      )
      .toBeGreaterThan(0.2);
    expect(artist.provider.audioRequests.length).toBeGreaterThan(0);

    const unfound = table.getByRole("listitem").filter({ hasText: hidden });
    await expect(unfound).toContainText("No audio found");
    await expect(
      table.getByRole("listitem").filter({ hasText: closing }),
    ).not.toContainText("Not found");

    await page.getByRole("button", { name: "Open queue", exact: true }).click();
    await expect(page.locator("#queue-panel[data-open]")).toBeVisible();
    const now = page.getByRole("region", { name: "Now playing", exact: true });
    await expect(now.locator("strong")).toHaveText(opening);
    const rows = page.locator("ol.queue-tracks > li");
    await expect(rows).toHaveCount(2);
    expect(await rows.locator("strong").allTextContents()).toEqual([
      interlude,
      closing,
    ]);
    expect(artist.provider.state.unexpected).toBe(0);
    expect(errors).toEqual([]);
  } finally {
    await artist.close();
  }
});

test("with automatic service search off, Play says why, links to the setting and searches nothing", async ({
  page,
  baseURL,
}) => {
  test.setTimeout(90_000);
  const artist = await connectCatalogArtist(page, baseURL!);
  const headers = { "x-hito-request": "1", origin: new URL(baseURL!).origin };
  const automation = (value: boolean) =>
    page.request.put("/api/v1/metadata/automation", {
      headers,
      data: {
        automatic_service_search: value,
      } satisfies Schema<"UpdateAutomaticLookups">,
    });
  expect((await automation(false)).status()).toBe(200);
  try {
    await page.goto(artist.path);
    await page
      .getByRole("link", { name: `Open ${catalogAlbum.groupTitle}` })
      .click();
    await page.getByRole("button", { name: "Play album" }).click();
    const note = page.getByRole("status").filter({
      hasText: "Search connected services automatically",
    });
    await expect(note).toBeVisible();
    await expect(
      note.getByRole("link", { name: "Turn it on in Settings" }),
    ).toHaveAttribute("href", "/settings?section=privacy");
    expect(artist.provider.state.searches).toEqual([]);
    expect(artist.provider.audioRequests).toEqual([]);
  } finally {
    await automation(true);
    await artist.close();
  }
});

test("the release card plays straight from its corner button and shows a discreet play control on a phone", async ({
  page,
  baseURL,
}) => {
  test.setTimeout(90_000);
  const artist = await connectCatalogArtist(page, baseURL!);
  try {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto(artist.path);
    const play = page.getByRole("button", {
      name: `Play ${catalogAlbum.groupTitle}`,
    });
    await expect(play).toBeVisible();
    await expect(page.getByRole("button", { name: "Open album" })).toHaveCount(
      0,
    );
    await expect(page.getByText("Not in your library")).toHaveCount(0);
    const cover = play
      .locator("xpath=ancestor::div[contains(@class,'shelf-card')]")
      .locator(".shelf-card-cover");
    const coverBox = (await cover.boundingBox())!;
    const playBox = (await play.boundingBox())!;
    expect(playBox.x + playBox.width).toBeLessThanOrEqual(
      coverBox.x + coverBox.width + 1,
    );
    expect(playBox.y + playBox.height).toBeLessThanOrEqual(
      coverBox.y + coverBox.height + 1,
    );
    expect(playBox.x).toBeGreaterThan(coverBox.x + coverBox.width / 2);
    await play.click();
    const audio = page.locator("audio[data-hito-player]");
    await expect
      .poll(() =>
        audio.evaluate((element: HTMLAudioElement) => element.currentTime),
      )
      .toBeGreaterThan(0.2);
  } finally {
    await artist.close();
  }
});

test("on a desktop the corner play button appears on hover and keyboard focus only", async ({
  page,
  baseURL,
}) => {
  test.setTimeout(90_000);
  const artist = await connectCatalogArtist(page, baseURL!);
  try {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto(artist.path);
    const open = page.getByRole("link", {
      name: `Open ${catalogAlbum.groupTitle}`,
    });
    const play = page.getByRole("button", {
      name: `Play ${catalogAlbum.groupTitle}`,
    });
    await expect(open).toBeVisible();
    await expect(play).toHaveCSS("opacity", "0");
    await open.hover();
    await expect(play).toHaveCSS("opacity", "1");
    await page.mouse.move(0, 0);
    await expect(play).toHaveCSS("opacity", "0");
    await play.focus();
    await expect(play).toHaveCSS("opacity", "1");
  } finally {
    await artist.close();
  }
});
