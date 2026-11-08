import { expect, test, type Page } from "@playwright/test";
import { enterAdmin, sharedAdminSession } from "./fixtures/admin-session";
import { stubTrackPage } from "./fixtures/track-fidelity";
import type { Schema } from "../src/shared/schema";

test.use({ deviceScaleFactor: 1, viewport: { width: 1440, height: 900 } });

async function libraryRecording(page: Page) {
  const listed = await page.request.get(
    "/api/v1/recordings?q=Fixture%20440%20Hz",
  );
  expect(listed.status()).toBe(200);
  const recording: Schema<"Recording"> | undefined = (
    await listed.json()
  ).items.find((item: Schema<"Recording">) => item.title === "Fixture 440 Hz");
  if (!recording) throw new Error("The scanned fixture library is required");
  return recording.id;
}

async function shot(page: Page, name: string) {
  const directory = process.env.BEST_GUESS_SHOTS;
  if (!directory) return;
  await page.waitForTimeout(400);
  await page.screenshot({ path: `${directory}/${name}.png` });
}

test("a source chosen from the provider ranking is marked as a best guess next to Fix source", async ({
  page,
}) => {
  test.setTimeout(120_000);
  if (!sharedAdminSession) await enterAdmin(page);
  await page.emulateMedia({ colorScheme: "dark" });
  const id = await libraryRecording(page);
  const base = `/recordings/${id}?tab=sources`;

  await stubTrackPage(page, id, { audio: "spotify" });
  await page.goto(base);
  const sources = page.getByRole("region", { name: "Playback source" });
  await expect(sources).toBeVisible();
  await expect(sources.getByText("Best guess")).toHaveCount(0);
  await shot(page, "sources-before");

  await page.unrouteAll({ behavior: "ignoreErrors" });
  await stubTrackPage(page, id, { audio: "guess" });
  await page.goto(base);
  await expect(sources).toBeVisible();
  const badge = sources.getByText("Best guess");
  await expect(badge).toBeVisible();
  await expect(badge).toHaveAttribute(
    "title",
    "Chosen from Jamendo's top result because no exact match was found.",
  );
  await shot(page, "sources-after");
  await sources.getByRole("button", { name: "Fix source" }).click();
  await expect(
    page.getByRole("heading", { name: "Add another source" }),
  ).toBeFocused();
});
