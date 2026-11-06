import { expect, type Locator, type Page } from "@playwright/test";

export function trackTitleLink(scope: Page | Locator, title: string) {
  return scope
    .getByRole("link", { name: title, exact: true })
    .and(scope.locator(".track-title, .episode-title"));
}

export async function openTrackMenu(
  scope: Page | Locator,
  title: string,
): Promise<Locator> {
  const page = "page" in scope ? scope.page() : scope;
  await scope
    .getByRole("button", { name: `Actions for ${title}`, exact: true })
    .click();
  const menu = page.getByRole("menu", { name: `Actions for ${title}` });
  await expect(menu).toBeVisible();
  return menu;
}

export async function addToPlaylistFromRow(
  scope: Page | Locator,
  title: string,
  playlist: string,
) {
  const page = "page" in scope ? scope.page() : scope;
  const menu = await openTrackMenu(scope, title);
  await menu.getByRole("menuitem", { name: "Add to playlist" }).click();
  await page
    .getByRole("menu", { name: "Add to playlist" })
    .getByRole("menuitem", { name: playlist, exact: true })
    .click();
  await expect(page.getByRole("menu")).toHaveCount(0);
}
