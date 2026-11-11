import type { Page } from "@playwright/test";

export async function openAllManage(page: Page) {
  const buttons = page.getByRole("button", { name: "Manage", exact: true });
  await buttons.first().waitFor();
  const count = await buttons.count();
  for (let index = 0; index < count; index++) {
    const button = buttons.nth(index);
    if ((await button.getAttribute("aria-expanded")) === "false")
      await button.click();
  }
}
