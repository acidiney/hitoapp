import { expect, test, type Locator } from "@playwright/test";
import { catalogAlbum } from "./fixtures/catalog-album-provider";
import { connectCatalogArtist } from "./fixtures/catalog-album-setup";

const phoneWidth = 390;

async function shownWhole(locator: Locator, width = phoneWidth) {
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
  expect(box.right).toBeLessThanOrEqual(width);
}

async function boxOf(locator: Locator) {
  const box = await locator.boundingBox();
  expect(box).not.toBeNull();
  return box!;
}

function apart(
  a: { x: number; y: number; width: number; height: number },
  b: { x: number; y: number; width: number; height: number },
) {
  return (
    a.x + a.width <= b.x ||
    b.x + b.width <= a.x ||
    a.y + a.height <= b.y ||
    b.y + b.height <= a.y
  );
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
    const note = page.getByText("From catalog", { exact: true });
    await shownWhole(note);
    const edition = page.getByRole("button", { name: /^Edition / });
    const noteBox = (await note.boundingBox())!;
    const editionBox = (await edition.boundingBox())!;
    expect(noteBox.y).toBeGreaterThanOrEqual(editionBox.y);
    expect(noteBox.y + noteBox.height).toBeLessThanOrEqual(
      editionBox.y + editionBox.height,
    );
    const firstRow = page
      .getByRole("list", { name: "Tracks" })
      .getByRole("listitem")
      .first();
    const rowBox = (await firstRow.boundingBox())!;
    expect(rowBox.y - (editionBox.y + editionBox.height)).toBeLessThanOrEqual(
      16,
    );
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= window.innerWidth,
      ),
    ).toBe(true);
  } finally {
    await artist.close();
  }
});

for (const width of [320, 390]) {
  test(`on a ${width} px phone an album with a saved track keeps its filters, a usable Edition button and the short note`, async ({
    page,
    baseURL,
  }) => {
    test.setTimeout(120_000);
    await page.setViewportSize({ width, height: 844 });
    const artist = await connectCatalogArtist(page, baseURL!);
    try {
      await page.goto(
        `${artist.path}/releases/${encodeURIComponent(catalogAlbum.groupId)}`,
      );
      const tracks = page.getByRole("list", { name: "Tracks" });
      await expect(tracks.getByRole("listitem")).toHaveCount(
        catalogAlbum.tracks.length,
      );
      const saved = catalogAlbum.tracks[0];
      const row = tracks.getByRole("listitem").filter({ hasText: saved });
      await row
        .getByRole("button", {
          name: `Add ${saved} to your library`,
          exact: true,
        })
        .click();
      await expect(
        row.getByRole("img", { name: "In your library", exact: true }),
      ).toBeVisible();
      const all = page.getByRole("button", {
        name: `All ${catalogAlbum.tracks.length}`,
        exact: true,
      });
      const mine = page.getByRole("button", {
        name: "In my library 1",
        exact: true,
      });
      const edition = page.getByRole("button", { name: /^Edition / });
      const label = edition.locator(".edition-chip-label");
      const chevron = edition.locator("svg");
      const note = page.getByText("From catalog", { exact: true });
      for (const part of [all, mine, edition, note])
        await shownWhole(part, width);
      await shownWhole(page.locator(".album-header .album-stats"), width);
      const editionBox = await boxOf(edition);
      const labelBox = await boxOf(label);
      const chevronBox = await boxOf(chevron);
      expect(labelBox.x).toBeGreaterThanOrEqual(editionBox.x);
      expect(chevronBox.x + chevronBox.width).toBeLessThanOrEqual(
        editionBox.x + editionBox.width,
      );
      expect(apart(labelBox, chevronBox)).toBe(true);
      const noteBox = await boxOf(note);
      expect(noteBox.y).toBeGreaterThanOrEqual(editionBox.y);
      expect(noteBox.y + noteBox.height).toBeLessThanOrEqual(
        editionBox.y + editionBox.height,
      );
      const parts = [await boxOf(all), await boxOf(mine), editionBox, noteBox];
      for (let i = 0; i < parts.length; i++)
        for (let j = i + 1; j < parts.length; j++)
          expect(apart(parts[i], parts[j])).toBe(true);
      const firstRow = await boxOf(tracks.getByRole("listitem").first());
      expect(
        firstRow.y - (editionBox.y + editionBox.height),
      ).toBeLessThanOrEqual(16);
      expect(
        await page.evaluate(
          () => document.documentElement.scrollWidth <= window.innerWidth,
        ),
      ).toBe(true);
    } finally {
      await artist.close();
    }
  });
}
