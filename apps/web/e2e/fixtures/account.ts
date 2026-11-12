import { expect, type Locator, type Page } from "@playwright/test";

export function accountOptionsButton(page: Page): Locator {
  return page.locator(".account").getByRole("button", {
    name: "Account options",
    exact: true,
  });
}

export async function openAccountOptions(page: Page): Promise<Locator> {
  const trigger = accountOptionsButton(page);
  if ((await trigger.getAttribute("aria-expanded")) !== "true")
    await trigger.click();
  const menu = page.getByRole("menu", { name: "Account options" });
  await expect(menu).toBeVisible();
  return menu;
}

async function chooseFromShell(
  page: Page,
  name: string,
  options: { noWaitAfter?: boolean } = {},
) {
  const menu = await openAccountOptions(page);
  await menu.getByRole("menuitem", { name, exact: true }).click(options);
}

export function signOutFromShell(
  page: Page,
  options: { noWaitAfter?: boolean } = {},
) {
  return chooseFromShell(page, "Sign out", options);
}

export function lockFromShell(page: Page) {
  return chooseFromShell(page, "Lock");
}

export function openShortcutsFromShell(page: Page) {
  return chooseFromShell(page, "Keyboard shortcuts");
}
