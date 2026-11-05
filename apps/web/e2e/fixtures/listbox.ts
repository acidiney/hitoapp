import { expect, type Locator } from "@playwright/test";

export async function chooseOption(
  combobox: Locator,
  option: string | number,
): Promise<void> {
  await combobox.click();
  const list = combobox.page().getByRole("listbox");
  await expect(list).toBeVisible();
  const options = list.getByRole("option");
  const target =
    typeof option === "number"
      ? options.nth(option)
      : options.getByText(option, { exact: true });
  await target.click();
  await expect(list).toHaveCount(0);
}
