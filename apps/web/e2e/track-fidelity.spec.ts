import { expect, test, type Page } from "@playwright/test";
import { enterAdmin, sharedAdminSession } from "./fixtures/admin-session";
import { stubTrackPage, syncedLyrics } from "./fixtures/track-fidelity";
import type { Schema } from "../src/shared/schema";

type View = "desktop-dark" | "desktop-light" | "phone-dark";
const sizes: Record<
  View,
  { width: number; height: number; scheme: "dark" | "light" }
> = {
  "desktop-dark": { width: 1280, height: 800, scheme: "dark" },
  "desktop-light": { width: 1280, height: 800, scheme: "light" },
  "phone-dark": { width: 375, height: 812, scheme: "dark" },
};

test.use({ deviceScaleFactor: 2, viewport: { width: 1280, height: 800 } });

async function shot(page: Page, name: string, view: View = "desktop-dark") {
  const directory = process.env.TRACK_SHOTS;
  if (!directory) return;
  const size = sizes[view];
  await page.emulateMedia({ colorScheme: size.scheme });
  await page.setViewportSize({ width: size.width, height: size.height });
  await page.waitForTimeout(500);
  await page.screenshot({ path: `${directory}/${name}.png` });
  await page.emulateMedia({ colorScheme: "dark" });
  await page.setViewportSize(sizes["desktop-dark"]);
}

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

test("the track page renders the mockup data screen by screen", async ({
  page,
}) => {
  test.setTimeout(180_000);
  if (!sharedAdminSession) await enterAdmin(page);
  await page.emulateMedia({ colorScheme: "dark" });
  const id = await libraryRecording(page);
  const base = `/recordings/${id}`;
  const header = page.locator(".track-header");

  await stubTrackPage(page, id, { audio: "local", lyrics: syncedLyrics });
  await page.goto(base);
  await expect(header.getByRole("heading", { level: 1 })).toHaveText(
    "Slow Tide",
  );
  await expect(header.getByText("In library")).toBeVisible();
  await expect(page.getByRole("tab", { name: /^Sources/ })).toContainText("1");
  await shot(page, "01-overview");
  await shot(page, "19-overview-light", "desktop-light");
  await shot(page, "p1-overview-phone", "phone-dark");

  await page.getByRole("tab", { name: /^Lyrics/ }).click();
  await page.getByRole("button", { name: "Choose lyric provider" }).click();
  await expect(page.getByRole("list", { name: "Lyric lines" })).toBeVisible();
  await expect(page.getByText("Synced", { exact: true })).toBeVisible();
  await shot(page, "03-lyrics-synced");

  await page.getByRole("tab", { name: /^Sources/ }).click();
  await expect(
    page.getByRole("region", { name: "Playback source" }),
  ).toBeVisible();
  await shot(page, "06-sources-local-ready");

  await page.getByRole("tab", { name: /^Metadata/ }).click();
  await expect(page.getByText("1 personal edit")).toBeVisible();
  await shot(page, "14-metadata-view");
  await page.getByRole("button", { name: "Edit track title" }).click();
  await expect(
    page.getByRole("textbox", { name: "Track title" }),
  ).toBeFocused();
  await shot(page, "15-metadata-edit-title");
  await shot(page, "p4-metadata-edit-phone", "phone-dark");
  await page.keyboard.press("Escape");

  await page.getByRole("tab", { name: /^History/ }).click();
  await expect(
    page.getByRole("region", { name: "Source observations" }),
  ).toBeVisible();
  await shot(page, "18-history");

  await page.unrouteAll({ behavior: "ignoreErrors" });
  await stubTrackPage(page, id, { audio: "local", lyrics: null });
  await page.goto(`${base}?tab=lyrics`);
  await page.getByRole("button", { name: "Choose lyric provider" }).click();
  await expect(
    page.getByText("No lyrics found for this recording."),
  ).toBeVisible();
  await shot(page, "05-lyrics-none-choose-provider");

  await page.unrouteAll({ behavior: "ignoreErrors" });
  await stubTrackPage(page, id, { audio: "none" });
  await page.goto(base);
  await expect(header.getByText("Catalog only")).toBeVisible();
  await expect(
    header.getByRole("button", { name: "Find audio" }),
  ).toBeVisible();
  await shot(page, "02-overview-no-audio");
  await header.getByRole("button", { name: "Find audio" }).click();
  const panel = page.getByRole("region", { name: "Find audio" });
  await expect(
    panel.getByRole("searchbox", { name: "Search music" }),
  ).toHaveValue("Slow Tide Nadia Ferreira");
  await panel.getByRole("button", { name: "Search", exact: true }).click();
  await expect(panel.getByRole("button", { name: "Review match" })).toHaveCount(
    3,
  );
  await shot(page, "10-sources-none-find-audio");
  await shot(page, "p2-sources-none-phone", "phone-dark");
  await panel.getByRole("button", { name: "Review match" }).first().click();
  const dialog = page.getByRole("dialog", { name: "Review source match" });
  await expect(dialog).toBeVisible();
  await shot(page, "11-sources-review-dialog");
  await shot(page, "p3-sources-review-dialog-phone", "phone-dark");
  await dialog.getByRole("button", { name: "Cancel" }).click();

  await page.unrouteAll({ behavior: "ignoreErrors" });
  await stubTrackPage(page, id, { audio: "spotify", downloading: true });
  await page.goto(`${base}?tab=sources`);
  await expect(header.getByText("Plays from Spotify")).toBeVisible();
  await expect(
    page.getByRole("progressbar", { name: "Download progress" }),
  ).toBeVisible();
  await shot(page, "08-sources-provider-downloading");

  await page.unrouteAll({ behavior: "ignoreErrors" });
  await stubTrackPage(page, id, { audio: "manual" });
  await page.goto(`${base}?tab=sources`);
  await expect(header.getByText("Pinned")).toBeVisible();
  await page.getByRole("radio", { name: "Local file" }).check();
  await expect(
    page.getByText("Selected, not saved yet. Use source to play from it."),
  ).toBeVisible();
  await shot(page, "12-sources-manual-choice");

  await page.unrouteAll({ behavior: "ignoreErrors" });
  await stubTrackPage(page, id, { audio: "failing" });
  await page.goto(`${base}?tab=sources`);
  await expect(
    page.getByText(
      "Hito could not check the current source choice. Try again.",
    ),
  ).toBeVisible();
  await shot(page, "13-sources-failures");
});
