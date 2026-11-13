import { expect, test, type Locator } from "@playwright/test";
import { catalogAlbum } from "./fixtures/catalog-album-provider";
import { connectCatalogArtist } from "./fixtures/catalog-album-setup";

const phoneWidth = 390;

async function shownWhole(locator: Locator) {
  await expect(locator).toBeVisible();
  const box = await locator.evaluate((element) => {
    const rect = element.getBoundingClientRect();
    return {
      left: rect.left,
      right: rect.right,
      clipped: element.scrollWidth > element.clientWidth + 1,
    };
  });
  expect(box.clipped).toBe(false);
  expect(box.left).toBeGreaterThanOrEqual(0);
  expect(box.right).toBeLessThanOrEqual(phoneWidth);
}

test("on a phone the album header shows its whole credit line and track list source", async ({
  page,
  baseURL,
}) => {
  test.setTimeout(120_000);
  await page.setViewportSize({ width: phoneWidth, height: 844 });
  const artist = await connectCatalogArtist(page, baseURL!);
  try {
    await page.goto(
      `${artist.path}/releases/${encodeURIComponent(catalogAlbum.groupId)}`,
    );
    await expect(
      page.getByRole("list", { name: "Tracks" }).getByRole("listitem"),
    ).toHaveCount(catalogAlbum.tracks.length);
    const stats = page.locator(".album-header .album-stats");
    await expect(stats).toContainText(`${catalogAlbum.tracks.length} tracks`);
    await shownWhole(stats);
    await shownWhole(page.getByText(/^Track list from /));
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= window.innerWidth,
      ),
    ).toBe(true);
  } finally {
    await artist.close();
  }
});
