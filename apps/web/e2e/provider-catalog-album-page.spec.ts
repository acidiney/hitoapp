import { expect, test } from "@playwright/test";
import { catalogAlbum } from "./fixtures/catalog-album-provider";
import { connectCatalogArtist } from "./fixtures/catalog-album-setup";
import { openTrackMenu } from "./fixtures/track-row";

const [opening, interlude] = catalogAlbum.tracks;

test("the artist page lists discography as album cards that open a real album page with edition rows", async ({
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
    await expect(
      page
        .getByRole("region", { name: "Discography", exact: true })
        .getByRole("list", { name: "Tracks" }),
    ).toHaveCount(0);
    await card.click();
    await expect(page).toHaveURL(/\/artist-groups\/[^/]+\/releases\//);
    await expect(
      page.getByRole("heading", { level: 1, name: catalogAlbum.groupTitle }),
    ).toBeVisible();

    const table = page.getByRole("list", { name: "Tracks" });
    await expect(table.getByRole("listitem")).toHaveCount(4);
    for (const title of catalogAlbum.tracks)
      await expect(
        table.getByRole("button", { name: `Play ${title}`, exact: true }),
      ).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Play album", exact: true }),
    ).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Shuffle album", exact: true }),
    ).toBeVisible();
    await expect(page.locator("header.album-header select")).toHaveCount(0);
    await expect(page.getByRole("button", { name: /^Edition / })).toBeVisible();
    await expect(page.getByText(/^Track list from /)).toBeVisible();

    await table
      .getByRole("button", { name: `Play ${interlude}`, exact: true })
      .click();
    const audio = page.locator("audio[data-hito-player]");
    await expect
      .poll(() =>
        audio.evaluate((element: HTMLAudioElement) => element.currentTime),
      )
      .toBeGreaterThan(0.2);

    const row = table.getByRole("listitem").filter({ hasText: interlude });
    await expect(
      row.getByRole("img", { name: "In your library", exact: true }),
    ).toBeVisible();

    const first = table.getByRole("listitem").filter({ hasText: opening });
    await first
      .getByRole("button", {
        name: `Add ${opening} to your library`,
        exact: true,
      })
      .click();
    await expect(
      first.getByRole("img", { name: "In your library", exact: true }),
    ).toBeVisible();
    const menu = await openTrackMenu(first, opening);
    await menu.getByRole("menuitem", { name: "Track details" }).click();
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
    const table = page.getByRole("list", { name: "Tracks" });
    await expect(table.getByRole("listitem")).toHaveCount(4);
    const first = [...reads];
    await page.goBack();
    await page
      .getByRole("link", { name: `Open ${catalogAlbum.groupTitle}` })
      .click();
    await expect(table.getByRole("listitem")).toHaveCount(4);
    expect(reads).toEqual(first);
  } finally {
    await artist.close();
  }
});

test("a track whose audio was found shows as in your library on the next visit and after a reload", async ({
  page,
  baseURL,
}) => {
  test.setTimeout(120_000);
  const artist = await connectCatalogArtist(page, baseURL!);
  try {
    await page.goto(artist.path);
    await page
      .getByRole("link", { name: `Open ${catalogAlbum.groupTitle}` })
      .click();
    const table = page.getByRole("list", { name: "Tracks" });
    await expect(table.getByRole("listitem")).toHaveCount(4);
    const first = table.getByRole("listitem").filter({ hasText: opening });
    await first
      .getByRole("button", {
        name: `Add ${opening} to your library`,
        exact: true,
      })
      .click();
    await expect(
      first.getByRole("img", { name: "In your library", exact: true }),
    ).toBeVisible();

    await page.goBack();
    await page
      .getByRole("link", { name: `Open ${catalogAlbum.groupTitle}` })
      .click();
    await expect(table.getByRole("listitem")).toHaveCount(4);
    const again = table.getByRole("listitem").filter({ hasText: opening });
    await expect(
      again.getByRole("img", { name: "In your library", exact: true }),
    ).toBeVisible();
    await expect(
      again.getByRole("button", {
        name: `Add ${opening} to your library`,
        exact: true,
      }),
    ).toHaveCount(0);

    await page.reload();
    const reloaded = page
      .getByRole("list", { name: "Tracks" })
      .getByRole("listitem")
      .filter({ hasText: opening });
    await expect(
      reloaded.getByRole("img", { name: "In your library", exact: true }),
    ).toBeVisible();
    await expect(
      reloaded.getByRole("button", {
        name: `Actions for ${opening}`,
        exact: true,
      }),
    ).toBeVisible();
  } finally {
    await artist.close();
  }
});
