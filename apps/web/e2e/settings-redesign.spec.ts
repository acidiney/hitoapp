import { expect, test } from "@playwright/test";
import { enterAdmin } from "./fixtures/admin-session";

test.use({ storageState: { cookies: [], origins: [] } });

test("settings deep links, legacy aliases and the grouped navigation work for an administrator", async ({
  page,
}) => {
  await enterAdmin(page);
  await page.goto("/settings?section=metadata");
  const nav = page.getByRole("navigation", { name: "Settings sections" });
  await expect(
    nav.getByRole("link", { name: "Music & providers", exact: true }),
  ).toHaveAttribute("aria-current", "page");
  await expect(
    page.getByRole("region", { name: "Metadata providers" }),
  ).toBeVisible();
  await expect(
    page.getByRole("region", { name: "Music folders" }),
  ).toBeVisible();
  await expect(nav.getByRole("link")).toHaveText([
    "Profile",
    "Privacy & lock",
    "Music & providers",
    "Plugins",
    "Playback",
    "Downloads & storage",
    "Accounts",
    "Updates",
    "About",
  ]);

  await page.goto("/settings?section=library");
  await expect(
    page.getByRole("region", { name: "Music folders" }),
  ).toBeVisible();

  await nav.getByRole("link", { name: "About", exact: true }).click();
  await expect(page).toHaveURL(/section=about$/);
  await expect(
    page.getByRole("heading", { name: "About", level: 2 }),
  ).toBeFocused();

  // phone drill-down
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/settings");
  await expect(nav).toBeVisible();
  await expect(page.getByRole("heading", { level: 2 })).toHaveCount(0);
  await expect(
    page.getByRole("link", { name: "Account and settings" }),
  ).toBeHidden();

  await nav.getByRole("link", { name: "Profile", exact: true }).click();
  await expect(nav).toBeHidden();
  await expect(
    page.getByRole("heading", { name: "Profile", level: 2 }),
  ).toBeVisible();
  await expect(page.getByRole("button", { name: "Sign out" })).toBeVisible();

  await page.getByRole("link", { name: "All settings" }).click();
  await expect(nav).toBeVisible();

  await page.goto("/podcasts");
  const subscribe = page.getByRole("button", {
    name: "Subscribe to a podcast",
    exact: true,
  });
  await subscribe.click();
  const dialog = page.getByRole("dialog", { name: "Subscribe to a podcast" });
  await expect(dialog).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
  await expect(subscribe).toBeFocused();
  await subscribe.click();
  await expect(dialog).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
  await expect(subscribe).toBeFocused();
});
