import { expect, test } from "@playwright/test";
import { connectCatalogArtist } from "./fixtures/catalog-album-setup";

test("opening a credit that is fully identified lands on the chosen artist page", async ({
  page,
  baseURL,
}) => {
  test.setTimeout(120_000);
  const artist = await connectCatalogArtist(page, baseURL!);
  try {
    expect(artist.anchor.artist_id).toBeTruthy();
    await page.goto(`/artists/${artist.anchor.artist_id}`);
    await expect(page).toHaveURL(/\/artist-groups\/[^/]+$/);
    await expect(page.getByRole("heading", { level: 1 })).toBeVisible();
  } finally {
    await artist.close();
  }
});

test("the album artist in the header is a link to that artist", async ({
  page,
}) => {
  const releases = (
    await (await page.request.get("/api/v1/releases?limit=50")).json()
  ).items as { id: string; artist_id: string | null; artist: string | null }[];
  const release = releases.find((item) => item.artist_id && item.artist);
  test.skip(!release, "No fixture album has an artist credit");
  await page.goto(`/albums/${release!.id}`);
  const link = page
    .locator(".album-header")
    .getByRole("link", { name: release!.artist! });
  await expect(link).toHaveAttribute(
    "href",
    new RegExp(`^/(artists|artist-groups)/`),
  );
  await link.focus();
  await expect(link).toBeFocused();
  await link.click();
  await expect(page).toHaveURL(/\/artist(s|-groups)\//);
});
