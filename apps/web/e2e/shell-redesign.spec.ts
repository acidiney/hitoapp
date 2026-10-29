import { expect, test } from "@playwright/test";
import { enterAdmin } from "./fixtures/admin-session";

test.use({ storageState: { cookies: [], origins: [] } });

test("the shell serves bundled fonts, offers three destinations and focuses search with the keyboard", async ({
  page,
}) => {
  await enterAdmin(page);
  const navigation = page.getByRole("navigation", { name: "Main navigation" });
  await expect(navigation.getByRole("link")).toHaveText(["Home", "Library"]);
  await expect(
    navigation.getByRole("button", { name: "Open search" }),
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
      await document.fonts.load('16px "Geist Variable"');
      return document.fonts.check('16px "Geist Variable"');
    }),
  ).toBe(true);

  await page.keyboard.press("Control+K");
  const search = page.getByRole("textbox", {
    name: "Search library",
    exact: true,
  });
  await expect(search).toBeFocused();

  await navigation.getByRole("link", { name: "Home", exact: true }).click();
  await expect(page).toHaveURL(/\/discover$/);
  await page.keyboard.press("Control+K");
  await expect(page).toHaveURL(/\/library$/);
  await expect(search).toBeFocused();
});

test("a phone shows a three-tab bar with the mini player above it", async ({
  page,
}) => {
  await enterAdmin(page);
  await page.setViewportSize({ width: 390, height: 844 });
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
  await expect(page.getByRole("button", { name: "Sign out" })).toBeVisible();
  await expect(
    page.getByRole("link", { name: "Account and settings" }),
  ).toBeVisible();
  const account = await page
    .getByRole("link", { name: "Account and settings" })
    .boundingBox();
  const searchBox = await page.locator(".search").boundingBox();
  expect(account!.y + account!.height).toBeLessThanOrEqual(searchBox!.y);
  for (const link of await navigation.getByRole("link").all()) {
    const box = await link.boundingBox();
    expect(box!.height).toBeGreaterThanOrEqual(44);
  }
  expect(
    (await navigation
      .getByRole("button", { name: "Open search" })
      .boundingBox())!.height,
  ).toBeGreaterThanOrEqual(44);
});
