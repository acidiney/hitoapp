import { expect, type Page } from "@playwright/test";

export async function enterEditMode(page: Page) {
  const edit = page.getByRole("button", { name: "Edit", exact: true });
  await expect(edit).toBeVisible();
  await edit.click();
  await expect(
    page.getByRole("button", { name: "Done", exact: true }),
  ).toBeVisible();
}
