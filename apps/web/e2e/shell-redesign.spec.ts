import {
  accountOptionsButton,
  openAccountOptions,
  openShortcutsFromShell,
} from "./fixtures/account";
import { expect, test } from "@playwright/test";
import { openSharedAdminSession } from "./fixtures/admin-session";

test("the shell serves bundled fonts, offers five destinations and focuses search with the keyboard", async ({
  page,
}) => {
  await openSharedAdminSession(page);
  const navigation = page.getByRole("navigation", { name: "Main navigation" });
  await expect(navigation.getByRole("link")).toHaveText([
    /^Home$/,
    /^Search/,
    /^Discover$/,
    /^Library$/,
    /^Plugins/,
  ]);
  await expect(
    navigation.getByRole("link", { name: /^Search/ }),
  ).toHaveAttribute("aria-keyshortcuts", /(Meta|Control)\+K/);
  await expect(
    page.getByRole("link", { name: "Account and settings" }),
  ).toBeVisible();

  const css = await (await page.request.get("/assets/app.css")).text();
  const font = css.match(/url\(([^)]*\.woff2)\)/)?.[1];
  expect(font).toBeTruthy();
  const served = await page.request.get(font!);
  expect(served.status()).toBe(200);
  expect(served.headers()["content-type"]).toContain("font/woff2");
  expect(
    await page.evaluate(async () => {
      await document.fonts.load('16px "Figtree Variable"');
      return document.fonts.check('16px "Figtree Variable"');
    }),
  ).toBe(true);

  await page.keyboard.press("Control+K");
  const search = page.getByRole("textbox", {
    name: "Search library",
    exact: true,
  });
  await expect(search).toBeFocused();

  await navigation.getByRole("link", { name: "Home", exact: true }).click();
  await expect(page).toHaveURL(/\/home$/);
  await expect(
    page.getByRole("heading", {
      level: 1,
      name: /^Good (morning|afternoon|evening), /,
    }),
  ).toBeVisible();
  await page.screenshot({ path: "/workspace/test-results/home-desktop.png" });
  const homeSearch = page.getByRole("textbox", {
    name: "Search",
    exact: true,
  });
  await expect(homeSearch).toHaveAttribute(
    "placeholder",
    "Search, or paste a playlist link",
  );
  await page.keyboard.press("Control+K");
  await expect(homeSearch).toBeFocused();

  await navigation.getByRole("link", { name: /^Search/ }).click();
  await expect(page).toHaveURL(/\/search$/);
  await expect(
    page.getByRole("group", { name: "Search scope" }).getByRole("button"),
  ).toHaveText(["All", "Tracks", "Albums", "Artists"]);
  const searchPage = page.getByRole("searchbox", { name: "Search" });
  await searchPage.fill("Fixture");
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/\/search\?q=Fixture$/);
  await expect(searchPage).toHaveValue("Fixture");
  await page.screenshot({ path: "/workspace/test-results/search-desktop.png" });

  await navigation.getByRole("link", { name: "Discover", exact: true }).click();
  await expect(page).toHaveURL(/\/discover$/);
  await page.screenshot({
    path: "/workspace/test-results/discover-desktop.png",
  });
  await expect(
    page.getByRole("textbox", { name: "Search", exact: true }),
  ).toHaveCount(0);
  await page.keyboard.press("Control+K");
  await expect(page).toHaveURL(/\/search$/);
  await expect(page.getByRole("searchbox", { name: "Search" })).toBeFocused();
});

test("on a desktop the top bar search sits in the same place on Home and Queue, while Settings and Search own their fields and Discover has none", async ({
  page,
}) => {
  await openSharedAdminSession(page);
  const places: { x: number; y: number; width: number; height: number }[] = [];
  for (const path of ["/home", "/queue"]) {
    await page.goto(path);
    const field = page.getByRole("textbox", { name: "Search", exact: true });
    await expect(field).toBeVisible();
    await expect(page.getByRole("searchbox")).toHaveCount(0);
    places.push((await field.boundingBox())!);
  }
  for (const place of places) {
    expect(place.x).toBeCloseTo(places[0].x, 0);
    expect(place.y).toBeCloseTo(places[0].y, 0);
    expect(place.height).toBeCloseTo(places[0].height, 0);
  }
  await page.goto("/settings");
  await expect(
    page.getByRole("combobox", { name: "Search settings" }),
  ).toBeVisible();
  await expect(
    page.getByRole("textbox", { name: "Search", exact: true }),
  ).toHaveCount(0);
  await page.goto("/search");
  await expect(page.getByRole("searchbox", { name: "Search" })).toBeVisible();
  await page.goto("/discover");
  await expect(
    page.getByRole("searchbox", { name: "Search", exact: true }),
  ).toHaveCount(0);
  await page.goto("/library");
  const library = (await page
    .getByRole("textbox", { name: "Search library", exact: true })
    .boundingBox())!;
  expect(library.x).toBeCloseTo(places[0].x, 0);
  expect(library.y).toBeCloseTo(places[0].y, 0);
  expect(library.height).toBeCloseTo(places[0].height, 0);
});

test("on a phone only the Search page shows a search field and Home links to it", async ({
  page,
}) => {
  await openSharedAdminSession(page);
  await page.setViewportSize({ width: 390, height: 844 });
  for (const path of ["/home", "/discover", "/settings"]) {
    await page.goto(path);
    await expect(page.getByRole("heading", { level: 1 }).first()).toBeVisible();
    await expect(page.locator("#shell-search")).toHaveCount(0);
  }
  await page.goto("/search");
  await expect(page.locator("#shell-search")).toHaveCount(1);
  await expect(page.getByRole("searchbox", { name: "Search" })).toBeVisible();
  await page.goto("/library");
  await expect(page.locator("#shell-search")).toHaveCount(0);
  await page.goto("/home");
  await expect(
    page.getByRole("link", { name: "Search, or paste a playlist link" }),
  ).toBeVisible();
});

test("a phone shows a five-tab bar with the mini player above it", async ({
  page,
}) => {
  await openSharedAdminSession(page);
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/home");
  await page.screenshot({ path: "/workspace/test-results/home-mobile.png" });
  await page.goto("/search?q=Fixture");
  await page.screenshot({ path: "/workspace/test-results/search-mobile.png" });
  await page.goto("/library");
  const navigation = page.getByRole("navigation", { name: "Main navigation" });
  const bar = await navigation.boundingBox();
  expect(bar).not.toBeNull();
  expect(Math.round(bar!.y + bar!.height)).toBe(844);
  expect(bar!.width).toBe(390);
  const player = await page
    .getByRole("contentinfo", { name: "Audio player" })
    .boundingBox();
  expect(player).not.toBeNull();
  expect(Math.round(player!.y + player!.height)).toBeLessThanOrEqual(
    Math.round(bar!.y) + 1,
  );
  await expect(
    page.getByRole("link", { name: "Account and settings" }),
  ).toBeHidden();
  await page.goto("/home");
  await expect(accountOptionsButton(page)).toBeHidden();
  await expect(
    page.getByRole("link", { name: "Account and settings" }),
  ).toBeVisible();
  for (const link of await navigation.getByRole("link").all()) {
    const box = await link.boundingBox();
    expect(box!.height).toBeGreaterThanOrEqual(44);
  }
});

test("the account bar shows the whole role and one options menu above the player", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openSharedAdminSession(page);
  await page.goto("/library");
  const role = page.locator(".account-name small");
  await expect(role).toHaveText("Administrator");
  expect(
    await role.evaluate((node) => node.scrollWidth <= node.clientWidth),
  ).toBe(true);
  await expect(page.locator(".account").getByRole("button")).toHaveCount(1);
  const menu = await openAccountOptions(page);
  const items = menu.getByRole("menuitem");
  await expect(items.first()).toBeFocused();
  await expect(items.filter({ hasText: "Sign out" })).toBeVisible();
  const box = await menu.boundingBox();
  const trigger = await accountOptionsButton(page).boundingBox();
  expect(box!.y + box!.height).toBeLessThanOrEqual(trigger!.y);
  await page.keyboard.press("Escape");
  await expect(menu).toBeHidden();
  await expect(accountOptionsButton(page)).toBeFocused();
  await openShortcutsFromShell(page);
  await expect(
    page.getByRole("dialog", { name: "Keyboard shortcuts" }),
  ).toBeVisible();
});
