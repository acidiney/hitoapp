import { expect, test } from "@playwright/test";
import { catalogAlbum } from "./fixtures/catalog-album-provider";
import { connectCatalogArtist } from "./fixtures/catalog-album-setup";

const stage = process.env.HITO_UX_STAGE ?? "after";
const sizes = [
  { name: "desktop-dark", width: 1280, height: 800, scheme: "dark" },
  { name: "desktop-light", width: 1280, height: 800, scheme: "light" },
  { name: "phone-dark", width: 375, height: 812, scheme: "dark" },
  { name: "phone-light", width: 375, height: 812, scheme: "light" },
] as const;

test.skip(process.env.HITO_UX_CAPTURE !== "1", "Set HITO_UX_CAPTURE=1");

test("capture the discography grid and the catalog album", async ({
  page,
  baseURL,
}) => {
  test.setTimeout(240_000);
  const artist = await connectCatalogArtist(page, baseURL!);
  try {
    for (const size of sizes) {
      await page.emulateMedia({ colorScheme: size.scheme });
      await page.setViewportSize({ width: size.width, height: size.height });
      await page.goto(artist.path);
      const open = page
        .getByRole(stage === "before" ? "button" : "link", {
          name: `Open ${catalogAlbum.groupTitle}`,
        })
        .first();
      await expect(open).toBeVisible();
      await page.waitForTimeout(800);
      const region = page.getByRole("region", {
        name: "Discography",
        exact: true,
      });
      await region.scrollIntoViewIfNeeded();
      await region.screenshot({
        path: `/workspace/test-results/discography-ux/artist-${stage}-${size.name}.png`,
      });
      await open.click();
      if (stage === "after") {
        await expect(page).toHaveURL(/\/releases\//);
        await expect(page.getByRole("heading", { level: 1 })).toBeVisible();
      }
      await expect(
        page.getByRole("list", { name: "Tracks" }).first(),
      ).toBeVisible();
      await page.waitForTimeout(800);
      await page.screenshot({
        path: `/workspace/test-results/discography-ux/album-${stage}-${size.name}.png`,
        fullPage: stage === "before",
      });
    }
  } finally {
    await artist.close();
  }
});
