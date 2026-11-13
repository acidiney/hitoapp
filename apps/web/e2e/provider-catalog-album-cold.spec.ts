import { expect, test, type BrowserContext, type Page } from "@playwright/test";
import { catalogAlbum } from "./fixtures/catalog-album-provider";
import { connectCatalogArtist } from "./fixtures/catalog-album-setup";
import { createMembers, memberPassword } from "./fixtures/member-accounts";

const albumPath = (artistPath: string) =>
  `${artistPath}/releases/${encodeURIComponent(catalogAlbum.groupId)}`;

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

test("a cold direct album link loads its editions and tracks while the artist profile is failing", async ({
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
    await expectTracks(page);
    await expect(
      page.getByRole("button", { name: "Play album" }),
    ).toBeVisible();
    await expect(page.getByRole("alert")).toHaveCount(0);
    const audioBodies: unknown[] = [];
    page.on("request", (request) => {
      if (new URL(request.url()).pathname.endsWith("/edition-track-audio"))
        audioBodies.push(request.postDataJSON());
    });
    const interlude = catalogAlbum.tracks[1];
    await page
      .getByRole("list", { name: "Tracks" })
      .getByRole("button", { name: `Play ${interlude}`, exact: true })
      .click();
    const player = page.locator("audio[data-hito-player]");
    await expect
      .poll(() =>
        player.evaluate((element: HTMLAudioElement) => element.currentTime),
      )
      .toBeGreaterThan(0.2);
    expect(audioBodies.length).toBeGreaterThan(0);
    for (const body of audioBodies)
      expect(body).toMatchObject({
        release_group_id: catalogAlbum.groupId,
        edition_id: catalogAlbum.editionId,
      });
    expect(reads).toEqual([]);
    expect(artist.provider.state.unexpected).toBe(0);
    expect(errors).toEqual([]);
  } finally {
    await artist.close();
  }
});

test("opening the album from the artist page keeps the release details and makes no album profile read", async ({
  page,
  baseURL,
}) => {
  test.setTimeout(120_000);
  const artist = await connectCatalogArtist(page, baseURL!);
  try {
    await page.goto(artist.path);
    const card = page.getByRole("link", {
      name: `Open ${catalogAlbum.groupTitle}`,
    });
    await expect(card).toBeVisible();
    const reads = profileReads(page);
    await card.click();
    await expect(page).toHaveURL(albumPath(artist.path));
    await expect(
      page.getByRole("heading", { level: 1, name: catalogAlbum.groupTitle }),
    ).toBeVisible();
    await expect(page.getByText(/2019/).first()).toBeVisible();
    await expectTracks(page);
    expect(reads).toEqual([]);
  } finally {
    await artist.close();
  }
});

test("another account cannot open someone else's album link", async ({
  page,
  browser,
  baseURL,
}) => {
  test.setTimeout(120_000);
  const artist = await connectCatalogArtist(page, baseURL!);
  let members: Awaited<ReturnType<typeof createMembers>> | undefined;
  let peer: BrowserContext | undefined;
  try {
    members = await createMembers(browser, baseURL!, "cold-album", 1);
    peer = await browser.newContext({ baseURL });
    const login = await peer.request.post("/api/v1/login", {
      headers: { "x-hito-request": "1", origin: new URL(baseURL!).origin },
      data: { username: members.names[0], password: memberPassword },
    });
    expect(login.status()).toBe(200);
    const other = await peer.newPage();
    const dispatched = artist.provider.state.routes.length;
    const response = await other.goto(albumPath(artist.path));
    expect(response?.status()).toBe(404);
    expect(artist.provider.state.routes.length).toBe(dispatched);
    for (const title of [catalogAlbum.groupTitle, ...catalogAlbum.tracks])
      await expect(other.getByText(title)).toHaveCount(0);
  } finally {
    try {
      await peer?.close();
    } finally {
      try {
        await members?.close();
      } finally {
        await artist.close();
      }
    }
  }
});

test("an album whose editions are unavailable says so and recovers on retry", async ({
  page,
  baseURL,
}) => {
  test.setTimeout(120_000);
  const artist = await connectCatalogArtist(page, baseURL!);
  try {
    artist.provider.state.failing.add("v1/catalog/release-groups/editions");
    await page.goto(albumPath(artist.path));
    const region = page.getByRole("region", { name: "Album" });
    await expect(region.getByRole("alert")).toBeVisible();
    await expect(page.getByRole("list", { name: "Tracks" })).toHaveCount(0);
    artist.provider.state.failing.clear();
    await region.getByRole("button", { name: "Try editions again" }).click();
    await expectTracks(page);
  } finally {
    await artist.close();
  }
});

test("an album whose provider cannot browse editions says so without reading the artist profile", async ({
  page,
  baseURL,
}) => {
  test.setTimeout(120_000);
  const artist = await connectCatalogArtist(page, baseURL!);
  try {
    artist.provider.state.editionsUnsupported = true;
    const reads = profileReads(page);
    await page.goto(albumPath(artist.path));
    const region = page.getByRole("region", { name: "Album" });
    await expect(region.getByRole("alert")).toHaveText(
      "This provider does not support edition browsing.",
    );
    await expect(page.getByRole("list", { name: "Tracks" })).toHaveCount(0);
    expect(reads).toEqual([]);
    expect(artist.provider.state.unexpected).toBe(0);
  } finally {
    await artist.close();
  }
});
