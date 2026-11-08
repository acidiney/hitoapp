import { expect, type Locator, type Page } from "@playwright/test";

export async function openPlaylistImport(page: Page): Promise<Locator> {
  await page.getByRole("button", { name: "Import from a link" }).click();
  const dialog = page.getByRole("dialog", { name: "Import a playlist" });
  await expect(dialog).toBeVisible();
  return dialog;
}

export async function openNewPlaylist(page: Page): Promise<Locator> {
  await page.getByRole("button", { name: "New playlist", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "New playlist" });
  await expect(dialog).toBeVisible();
  return dialog;
}

export async function createPlaylist(page: Page, name: string) {
  const dialog = await openNewPlaylist(page);
  await dialog.getByRole("textbox", { name: "Name" }).fill(name);
  await dialog.getByRole("button", { name: "Create playlist" }).click();
  await expect(dialog).toBeHidden();
}
