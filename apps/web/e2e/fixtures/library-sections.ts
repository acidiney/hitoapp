import type { Page } from "@playwright/test";

export async function openLibrarySection(page: Page, name: string) {
  const sections = page.getByRole("navigation", {
    name: "Library sections",
    exact: true,
  });
  if (!(await sections.isVisible()))
    await page
      .getByRole("navigation", { name: "Main navigation", exact: true })
      .getByRole("link", { name: "Library", exact: true })
      .click();
  await sections.getByRole("link", { name, exact: true }).click();
}
