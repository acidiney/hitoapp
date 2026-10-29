import type { Page } from "@playwright/test";

/** Opens a collection page from Biblioteca, entering Biblioteca first when the section strip is not on screen. */
export async function openLibrarySection(page: Page, name: string) {
  const sections = page.getByRole("navigation", {
    name: "Library sections",
    exact: true,
  });
  if (!(await sections.isVisible()))
    await page
      .getByRole("navigation", { name: "Main navigation", exact: true })
      .getByRole("link", { name: "Biblioteca", exact: true })
      .click();
  await sections.getByRole("link", { name, exact: true }).click();
}
