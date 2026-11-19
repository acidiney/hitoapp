import { expect, test, type Page } from "@playwright/test";
import { catalogAlbum } from "./fixtures/catalog-album-provider";
import { connectCatalogArtist } from "./fixtures/catalog-album-setup";

async function shot(page: Page, name: string) {
  const directory = process.env.ALBUM_ADDITION_SHOTS;
  if (!directory) return;
  await page.emulateMedia({ colorScheme: "dark" });
  for (const [size, viewport] of [
    ["desktop", { width: 1280, height: 900 }],
    ["phone", { width: 390, height: 844 }],
  ] as const) {
    await page.setViewportSize(viewport);
    await page.waitForTimeout(300);
    await page.screenshot({ path: `${directory}/${name}-${size}.png` });
  }
  await page.setViewportSize({ width: 1280, height: 900 });
}

test("adding a whole album brings every track into the library and undo takes them out", async ({
  page,
  baseURL,
}) => {
  test.setTimeout(120_000);
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  const artist = await connectCatalogArtist(page, baseURL!);
  await page.goto(artist.path);
  await page
    .getByRole("link", { name: `Open ${catalogAlbum.groupTitle}` })
    .click();
  const table = page.getByRole("list", { name: "Tracks" });
  await expect(table.getByRole("listitem")).toHaveCount(
    catalogAlbum.tracks.length,
  );
  const add = page.getByRole("button", { name: /^Add (album|the other \d+)$/ });
  await expect(add).toBeVisible();
  await shot(page, "album-before-add");

  await add.click();
  await expect(
    page
      .getByRole("status")
      .filter({ hasText: /^Added \d+ tracks? to your library\./ }),
  ).toBeVisible();
  for (const title of catalogAlbum.tracks)
    await expect(
      table
        .getByRole("listitem")
        .filter({ hasText: title })
        .getByRole("img", { name: "In your library", exact: true }),
    ).toBeVisible();
  await expect(
    page.getByRole("button", { name: /^Add (album|the other)/ }),
  ).toHaveCount(0);
  await shot(page, "album-after-add");

  await page.getByRole("button", { name: "Undo", exact: true }).click();
  await expect(
    page.getByRole("status").filter({ hasText: /^Removed \d+ tracks?\./ }),
  ).toBeVisible();
  await expect(add).toBeVisible();
  await shot(page, "album-after-undo");
  expect(errors).toEqual([]);
});
