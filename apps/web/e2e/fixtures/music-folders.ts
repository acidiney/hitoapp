import { expect, type Page } from "@playwright/test";

export async function addMusicFolders(page: Page, folders: string[]) {
  await page.goto("/settings?section=music");
  for (const folder of folders) {
    await page
      .getByRole("button", { name: "Add music folder", exact: true })
      .click();
    const picker = page.getByRole("dialog", {
      name: "Add a music folder",
      exact: true,
    });
    await picker
      .getByRole("button", { name: `Open ${folder}`, exact: true })
      .click();
    await picker
      .getByRole("button", { name: "Use this folder", exact: true })
      .click();
    await expect(picker).toBeHidden();
  }
  for (const folder of folders)
    await expect(
      page
        .getByRole("list", { name: "Connected music folders" })
        .getByRole("listitem")
        .filter({
          has: page.getByRole("heading", { name: folder, exact: true }),
        })
        .filter({ hasText: "1 file read" }),
    ).toHaveCount(1, { timeout: 20_000 });
}
