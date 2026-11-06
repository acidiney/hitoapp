import type { Page } from "@playwright/test";

const showAll =
  '.track-panel:not([data-active="true"]) { display: block !important; }';

export async function showAllTrackPanels(page: Page) {
  await page.addStyleTag({ content: showAll });
}

export async function openTrackEditors(page: Page) {
  await showAllTrackPanels(page);
  for (const field of ["title", "artist"]) {
    const pen = page.getByRole("button", { name: `Edit track ${field}` });
    if (await pen.isVisible()) await pen.click();
  }
}

export async function openMetadataEditors(page: Page) {
  await page.getByRole("tab", { name: /^Metadata/ }).click();
  for (const field of ["title", "artist"]) {
    const pen = page.getByRole("button", { name: `Edit track ${field}` });
    if (await pen.isVisible()) await pen.click();
  }
}
