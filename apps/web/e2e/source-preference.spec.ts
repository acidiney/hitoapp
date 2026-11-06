import { expect, test, type Page } from "@playwright/test";
import type { Schema } from "../src/shared/schema";

const headers = (baseURL: string) => ({
  "x-hito-request": "1",
  origin: new URL(baseURL).origin,
});

async function order(page: Page) {
  const list = page.getByRole("list", { name: "Source order" });
  await expect(list).toBeVisible();
  return list.getByRole("heading", { level: 4 }).allTextContents();
}

test("an account reorders its sources and turns the fallback off, and both survive a reload", async ({
  page,
  baseURL,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.goto("/settings?section=playback");
  const card = page.locator("#setting-source-preference");
  await expect(
    card.getByRole("heading", { name: "Source preference" }),
  ).toBeVisible();
  const before = await order(page);
  expect(before.slice(0, 2)).toEqual(["Local files", "Saved copies"]);
  const fallback = card.getByRole("switch", {
    name: "Try the next source if one fails",
  });
  await expect(fallback).toHaveAttribute("aria-checked", "true");
  const save = card.getByRole("button", { name: "Save source preference" });
  await expect(save).toBeDisabled();

  await card.getByRole("button", { name: "Move up Saved copies" }).click();
  await fallback.click();
  expect((await order(page)).slice(0, 2)).toEqual([
    "Saved copies",
    "Local files",
  ]);
  await save.click();
  await expect(page.getByText("Source preference saved.")).toBeVisible();

  await page.reload();
  expect((await order(page)).slice(0, 2)).toEqual([
    "Saved copies",
    "Local files",
  ]);
  await expect(
    page
      .locator("#setting-source-preference")
      .getByRole("switch", { name: "Try the next source if one fails" }),
  ).toHaveAttribute("aria-checked", "false");
  const stored: Schema<"SourcePreferences"> = await (
    await page.request.get("/api/v1/source-preferences")
  ).json();
  expect(stored.revision).toBe(1);
  expect(stored.fallback).toBe(false);
  expect(stored.entries.slice(0, 2)).toEqual([
    { kind: "saved_copies" },
    { kind: "local_files" },
  ]);

  const restored = await page.request.put("/api/v1/source-preferences", {
    headers: headers(baseURL!),
    data: {
      revision: stored.revision,
      fallback: true,
      entries: [
        { kind: "local_files" },
        { kind: "saved_copies" },
        ...stored.entries.slice(2),
      ],
    },
  });
  expect(restored.ok()).toBe(true);
  expect(errors).toEqual([]);
});
