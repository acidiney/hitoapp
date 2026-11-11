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

test("on a desktop the Recently added albums wrap into a grid instead of scrolling sideways", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await homeWithTwelveAlbums(page);
  const grid = page
    .getByRole("region", { name: "Recently added" })
    .locator("ul.home-grid");
  await expect(grid.locator("li")).toHaveCount(12);
  const tops = await grid
    .locator("li")
    .evaluateAll((items) => [
      ...new Set(
        items.map((item) => Math.round(item.getBoundingClientRect().top)),
      ),
    ]);
  expect(tops.length).toBeGreaterThan(1);
  expect(
    await grid.evaluate(
      (element) => element.scrollWidth <= element.clientWidth,
    ),
  ).toBe(true);
});

test("on a phone the albums stay in the page width", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await homeWithTwelveAlbums(page);
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
});
