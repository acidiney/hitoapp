import type { Page } from "@playwright/test";

/** Turns on edit mode for an editable playlist when it is not already on. */
export async function enterEditMode(page: Page) {
  const edit = page.getByRole("button", { name: "Edit", exact: true });
  if (await edit.isVisible()) await edit.click();
}
