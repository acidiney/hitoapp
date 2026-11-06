import { expect, test } from "@playwright/test";
import { catalogAlbum } from "./fixtures/catalog-album-provider";
import { connectCatalogArtist } from "./fixtures/catalog-album-setup";

const [opening, interlude] = catalogAlbum.tracks;

test("the artist page lists discography as album cards that open a real album page with standard rows", async ({
  page,
  baseURL,
}) => {
  test.setTimeout(120_000);
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  const artist = await connectCatalogArtist(page, baseURL!);
  try {
    await page.goto(artist.path);
    const card = page.getByRole("link", {
      name: `Open ${catalogAlbum.groupTitle}`,
    });
    await expect(card).toBeVisible();
    await expect(page.getByRole("table", { name: "Tracks" })).toHaveCount(0);
    await card.click();
    await expect(page).toHaveURL(/\/artist-groups\/[^/]+\/releases\//);
    await expect(
      page.getByRole("heading", { level: 1, name: catalogAlbum.groupTitle }),
    ).toBeVisible();

    const table = page.getByRole("table", { name: "Tracks" });
    await expect(table.getByRole("row")).toHaveCount(5);
    for (const title of catalogAlbum.tracks)
      await expect(
        table.getByRole("button", { name: `Play ${title}`, exact: true }),
      ).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Play album" }),
    ).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Shuffle album" }),
    ).toBeVisible();
    await expect(page.locator("select")).toHaveCount(0);

    await table
      .getByRole("button", { name: `Play ${interlude}`, exact: true })
      .click();
    const audio = page.locator("audio[data-hito-player]");
    await expect
      .poll(() =>
        audio.evaluate((element: HTMLAudioElement) => element.currentTime),
      )
      .toBeGreaterThan(0.2);

    const row = table.getByRole("row").filter({ hasText: interlude });
    await expect(
      row.getByRole("button", { name: `Like ${interlude}`, exact: true }),
    ).toBeVisible();
    await row
      .getByRole("button", { name: `Like ${interlude}`, exact: true })
      .click();
    await expect(
      row.getByRole("button", { name: `Unlike ${interlude}`, exact: true }),
    ).toBeVisible();

    const first = table.getByRole("row").filter({ hasText: opening });
    await first
      .getByRole("button", { name: `Find audio for ${opening}`, exact: true })
      .click();
    await expect(
      first.getByRole("button", { name: `Like ${opening}`, exact: true }),
    ).toBeVisible();
    await first
      .getByRole("link", { name: `Details for ${opening}`, exact: true })
      .click();
    await expect(page).toHaveURL(/\/recordings\/[^/]+$/);
    await expect(
      page.getByRole("heading", { level: 1, name: opening }),
    ).toBeVisible();
    expect(errors).toEqual([]);
  } finally {
    await artist.close();
  }
});

test("going back to the artist page and into the album again does not ask the provider twice", async ({
  page,
  baseURL,
}) => {
  test.setTimeout(120_000);
  const artist = await connectCatalogArtist(page, baseURL!);
  const reads: string[] = [];
  page.on("request", (request) => {
    const path = new URL(request.url()).pathname;
    if (/\/(discography|release-group-editions|edition-track-list)$/.test(path))
      reads.push(path.split("/").pop()!);
  });
  try {
    await page.goto(artist.path);
    await page
      .getByRole("link", { name: `Open ${catalogAlbum.groupTitle}` })
      .click();
    const table = page.getByRole("table", { name: "Tracks" });
    await expect(table.getByRole("row")).toHaveCount(5);
    const first = [...reads];
    await page.goBack();
    await page
      .getByRole("link", { name: `Open ${catalogAlbum.groupTitle}` })
      .click();
    await expect(table.getByRole("row")).toHaveCount(5);
    expect(reads).toEqual(first);
  } finally {
    await artist.close();
  }
});
