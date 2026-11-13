import { expect, test, type Page } from "@playwright/test";
import { catalogAlbum } from "./fixtures/catalog-album-provider";
import { connectCatalogArtist } from "./fixtures/catalog-album-setup";

const albumPath = (artistPath: string) =>
  `${artistPath}/releases/${encodeURIComponent(catalogAlbum.groupId)}`;
const eyebrow = "Album · 2019";

function profileReads(page: Page) {
  const reads: string[] = [];
  page.on("request", (request) => {
    if (new URL(request.url()).pathname.endsWith("/profile"))
      reads.push(request.url());
  });
  return reads;
}

async function expectTracks(page: Page) {
  const table = page.getByRole("list", { name: "Tracks" });
  await expect(table.getByRole("listitem")).toHaveCount(
    catalogAlbum.tracks.length,
  );
  for (const title of catalogAlbum.tracks)
    await expect(
      table.getByRole("button", { name: `Play ${title}`, exact: true }),
    ).toBeVisible();
}

test("a cold direct album link shows the release group's type and year from the provider while the artist profile fails", async ({
  page,
  baseURL,
}) => {
  test.setTimeout(120_000);
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  const artist = await connectCatalogArtist(page, baseURL!);
  try {
    artist.provider.state.failing.add("v1/catalog/artists/profile");
    const reads = profileReads(page);
    await page.goto(albumPath(artist.path));
    await expect(
      page.getByRole("heading", { level: 1, name: catalogAlbum.groupTitle }),
    ).toBeVisible();
    await expect(page.getByText(eyebrow, { exact: true })).toBeVisible();
    await expectTracks(page);
    await page
      .getByRole("list", { name: "Tracks" })
      .getByRole("button", { name: `Play ${catalogAlbum.tracks[1]}` })
      .click();
    const player = page.locator("audio[data-hito-player]");
    await expect
      .poll(() =>
        player.evaluate((element: HTMLAudioElement) => element.currentTime),
      )
      .toBeGreaterThan(0.2);
    expect(artist.provider.state.groupRequests).toEqual([catalogAlbum.groupId]);
    expect(reads).toEqual([]);
    expect(artist.provider.state.unexpected).toBe(0);
    expect(errors).toEqual([]);
  } finally {
    await artist.close();
  }
});

test("a release group read held by the provider does not hold back the editions, tracks or playback", async ({
  page,
  baseURL,
}) => {
  test.setTimeout(120_000);
  const artist = await connectCatalogArtist(page, baseURL!);
  try {
    artist.provider.state.failing.add("v1/catalog/artists/profile");
    artist.provider.state.groupDetail = "held";
    await page.goto(albumPath(artist.path));
    await expectTracks(page);
    await expect(
      page.getByRole("heading", { level: 1, name: catalogAlbum.groupTitle }),
    ).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Play album", exact: true }),
    ).toBeEnabled();
    await expect(page.getByText(eyebrow, { exact: true })).toHaveCount(0);
    await expect
      .poll(() => artist.provider.state.groupRequests)
      .toEqual([catalogAlbum.groupId]);
    expect(artist.provider.state.unexpected).toBe(0);
  } finally {
    await artist.close();
  }
});

test("a provider without release group details keeps the edition title and asks for no details", async ({
  page,
  baseURL,
}) => {
  test.setTimeout(120_000);
  const artist = await connectCatalogArtist(page, baseURL!);
  try {
    artist.provider.state.failing.add("v1/catalog/artists/profile");
    artist.provider.state.groupDetail = "legacy";
    await page.goto(albumPath(artist.path));
    await expectTracks(page);
    await expect(
      page.getByRole("heading", { level: 1, name: catalogAlbum.groupTitle }),
    ).toBeVisible();
    await expect(page.getByText(eyebrow, { exact: true })).toHaveCount(0);
    await expect(page.getByRole("alert")).toHaveCount(0);
    await expect
      .poll(() =>
        artist.provider.state.routes.includes(
          "v1/catalog/release-groups/capabilities",
        ),
      )
      .toBe(true);
    expect(artist.provider.state.groupRequests).toEqual([]);
    expect(artist.provider.state.unexpected).toBe(0);
  } finally {
    await artist.close();
  }
});
