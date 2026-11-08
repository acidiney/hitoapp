import { expect, test, type Page } from "@playwright/test";
import { openSharedAdminSession } from "./fixtures/admin-session";

const albums = Array.from({ length: 12 }, (_, index) => ({
  id: `shelf-album-${index}`,
  title: `Shelf album ${index}`,
  artist: "Shelf artist",
  inferred_artist: null,
}));

async function homeWithTwelveAlbums(page: Page) {
  await page.route("**/home", async (route) => {
    const request = route.request();
    if (request.headers()["x-inertia"] !== "true") return route.continue();
    const response = await route.fetch();
    const body = await response.json();
    body.props.home = {
      ...body.props.home,
      has_music: true,
      recently_added: albums,
    };
    await route.fulfill({ response, json: body });
  });
  await openSharedAdminSession(page);
  await page
    .getByRole("navigation", { name: "Main navigation" })
    .getByRole("link", { name: "Home", exact: true })
    .click();
  await expect(page).toHaveURL(/\/home$/);
}

const scrolled = (page: Page) =>
  page
    .getByRole("region", { name: "Recently added" })
    .locator("ul.shelf-row")
    .evaluate((row) => row.scrollLeft);

test("a clipped Home shelf on a desktop fades its edge and scrolls with previous and next buttons", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await homeWithTwelveAlbums(page);
  const shelf = page.getByRole("region", { name: "Recently added" });
  const row = shelf.locator("ul.shelf-row");
  const next = shelf.getByRole("button", { name: "Next in Recently added" });
  const previous = shelf.getByRole("button", {
    name: "Previous in Recently added",
  });
  await expect(row).toHaveAttribute("data-fade-end", "");
  await expect(next).toBeVisible();
  await expect(previous).toHaveCount(0);
  await next.click();
  await expect.poll(() => scrolled(page)).toBeGreaterThan(200);
  await expect(previous).toBeVisible();
  await expect(row).toHaveAttribute("data-fade-start", "");
  for (let step = 0; step < 6 && (await next.count()) > 0; step += 1) {
    await next.click();
    await page.waitForTimeout(400);
  }
  await expect(next).toHaveCount(0);
  await expect(row).not.toHaveAttribute("data-fade-end", "");
  await previous.click();
  await expect.poll(() => scrolled(page)).toBeGreaterThan(0);
});

test("on a phone the shelf swipes natively and shows no buttons, only the edge fade", async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await homeWithTwelveAlbums(page);
  const shelf = page.getByRole("region", { name: "Recently added" });
  await expect(shelf.locator("ul.shelf-row")).toHaveAttribute(
    "data-fade-end",
    "",
  );
  await expect(
    shelf.getByRole("button", { name: "Next in Recently added" }),
  ).toBeHidden();
});
