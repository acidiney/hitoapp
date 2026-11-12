import { mkdir } from "node:fs/promises";
import { expect, test, type Page } from "@playwright/test";
import { enterAdmin, sharedAdminSession } from "./fixtures/admin-session";
import type { Schema } from "../src/shared/schema";

const identity = {
  selectable: true,
  selector: {
    kind: "reviewed",
    identity_id: "33333333-3333-4333-8333-333333333333",
    identity_revision: 1,
  },
  installation_id: "11111111-1111-4111-8111-111111111111",
  installation_revision: 4,
  provider_recording: { id: "tablet-1", revision: null },
  label: "LRCLIB lyrics",
} satisfies Schema<"RecordingLyricIdentity">;

const lines = [
  "Should auld acquaintance be forgot,",
  "And never brought to mind?",
  "Should auld acquaintance be forgot,",
  "And auld lang syne!",
  "For auld lang syne, my dear,",
  "For auld lang syne,",
];

const sizes = [
  { width: 768, height: 1024 },
  { width: 862, height: 854 },
  { width: 1024, height: 768 },
  { width: 1180, height: 820 },
  { width: 1440, height: 900 },
];

async function mockSyncedLyrics(
  page: Page,
  recording: string,
  durationMs: number,
) {
  const base = `**/api/v1/recordings/${recording}/lyrics`;
  await page.route(`${base}/identities`, (route) =>
    route.fulfill({ json: { recording_id: recording, items: [identity] } }),
  );
  await page.route(`${base}/services`, (route) =>
    route.fulfill({ json: { recording_id: recording, items: [] } }),
  );
  await page.route(`${base}/identities/read`, (route) =>
    route.fulfill({
      json: {
        status: "ready_v2",
        recording_id: recording,
        selector: identity.selector,
        provider_recording: identity.provider_recording,
        document_digest: "0",
        document: {
          recording: identity.provider_recording,
          lyrics: {
            id: "doc",
            rendition_id: "r",
            duration_ms: durationMs,
            content: {
              kind: "anchored",
              lines: [
                { start_ms: 0, text: "" },
                ...lines.map((text, index) => ({
                  start_ms: 500 + index * 6000,
                  text,
                })),
                { start_ms: 40_000, text: "" },
              ],
            },
          },
        } as unknown,
      },
    }),
  );
}

test("Now playing shows the track, lyrics and Up next, Source and Speed at every width from 768 to 1440", async ({
  page,
}) => {
  if (!sharedAdminSession) await enterAdmin(page);
  await page.emulateMedia({ colorScheme: "dark" });
  await page.setViewportSize({ width: 1440, height: 900 });
  const listed = await page.request.get(
    "/api/v1/recordings?q=Fixture%20440%20Hz",
  );
  const recording: Schema<"Recording"> = (await listed.json()).items.find(
    (item: Schema<"Recording">) => item.title === "Fixture 440 Hz",
  );
  await mockSyncedLyrics(page, recording.id, recording.duration_ms);
  await page.goto(`/recordings/${recording.id}`);
  await page
    .locator(".track-header")
    .getByRole("button", { name: "Play", exact: true })
    .click();
  await page
    .getByRole("button", { name: /Open now playing|Expand player/ })
    .first()
    .click();
  const dialog = page.getByRole("dialog", { name: "Now playing" });
  const lyrics = dialog.getByRole("region", { name: "Lyrics", exact: true });
  await expect(lyrics.locator('[aria-current="true"]')).toBeVisible({
    timeout: 15_000,
  });
  const directory = process.env.TRACK_SHOTS ?? "test-results/np-tablet";
  await mkdir(directory, { recursive: true });
  for (const size of sizes) {
    await page.setViewportSize(size);
    await page.waitForTimeout(400);
    await page.screenshot({
      path: `${directory}/${process.env.SHOT_PREFIX ?? "after"}-${size.width}x${size.height}.png`,
    });
    const report = await dialog.evaluate((element) => {
      const root = element.getBoundingClientRect();
      const inside = (selector: string) => {
        const node = element.querySelector(selector);
        if (!node) return false;
        const box = node.getBoundingClientRect();
        return (
          box.width > 40 &&
          box.height > 40 &&
          box.left >= root.left - 1 &&
          box.right <= root.right + 1 &&
          box.top >= root.top - 1 &&
          box.bottom <= root.bottom + 1
        );
      };
      return {
        sideways: element.scrollWidth - element.clientWidth,
        below: element.scrollHeight - element.clientHeight,
        track: inside(".np-track"),
        lyrics: inside(".np-lyrics"),
        tabs: inside(".np-panels"),
        aside: inside(".np-aside"),
      };
    });
    const wide = size.width >= 1252;
    expect(report.sideways, `${size.width}: sideways`).toBeLessThanOrEqual(0);
    expect(report.below, `${size.width}: scrolls down`).toBeLessThanOrEqual(0);
    expect(report.track, `${size.width}: track`).toBe(true);
    expect(report.lyrics, `${size.width}: lyrics`).toBe(true);
    expect(report.aside, `${size.width}: aside`).toBe(wide);
    expect(report.tabs, `${size.width}: tabs`).toBe(!wide);
    if (!wide) {
      await dialog.getByRole("button", { name: "Source", exact: true }).click();
      await expect(
        dialog.getByRole("region", { name: "This source" }),
      ).toBeVisible();
      await page.waitForTimeout(400);
      await page.screenshot({
        path: `${directory}/${process.env.SHOT_PREFIX ?? "after"}-source-${size.width}x${size.height}.png`,
      });
      await dialog.getByRole("button", { name: "Lyrics", exact: true }).click();
      await expect(lyrics).toBeVisible();
    }
  }
});
