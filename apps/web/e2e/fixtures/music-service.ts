import type { Locator, Page } from "@playwright/test";
import type { Schema } from "../../src/shared/schema";

export async function chooseMusicService(
  page: Page,
  scope: Locator | Page,
  connectionId: string,
) {
  const response = await page.request.get("/api/v1/plugins");
  const settings: Schema<"PluginSettings"> = await response.json();
  const connection = settings.installations.find(
    (item) => item.id === connectionId,
  );
  if (!connection) throw new Error("Music service connection is not installed");
  const service = settings.services.find(
    (item) => item.id === connection.service_id,
  )?.name;
  const label =
    service && service !== connection.manifest.name
      ? `${service} · ${connection.manifest.name}`
      : connection.manifest.name;
  const control = scope.getByRole("combobox", {
    name: "Music service",
    exact: true,
  });
  await control.click();
  await scope.getByRole("option", { name: label, exact: true }).click();
}
