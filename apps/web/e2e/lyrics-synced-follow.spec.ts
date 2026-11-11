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
  provider_recording: { id: "follow-1", revision: null },
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

function mock(page: Page, recording: string, durationMs: number) {
  const base = `**/api/v1/recordings/${recording}/lyrics`;
  return Promise.all([
    page.route(`${base}/identities`, (route) =>
      route.fulfill({ json: { recording_id: recording, items: [identity] } }),
    ),
    page.route(`${base}/services`, (route) =>
      route.fulfill({ json: { recording_id: recording, items: [] } }),
    ),
    page.route(`${base}/identities/read`, (route) =>
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
          },
        } satisfies Schema<"SelectedLyricsLookup">,
      }),
    ),
  ]);
}

async function recordingOf(page: Page) {
  const listed = await page.request.get(
    "/api/v1/recordings?q=Fixture%20440%20Hz",
  );
  const found: Schema<"Recording"> = (await listed.json()).items.find(
    (item: Schema<"Recording">) => item.title === "Fixture 440 Hz",
  );
  return found;
}

async function shot(page: Page, name: string) {
  const directory = process.env.TRACK_SHOTS;
  if (!directory) return;
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.waitForTimeout(500);
  await page.screenshot({ path: `${directory}/${name}-desktop.png` });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.waitForTimeout(500);
  await page.screenshot({ path: `${directory}/${name}-phone.png` });
  await page.setViewportSize({ width: 1440, height: 900 });
}

test("Track Details lyrics follow the playing music when the durations match", async ({
  page,
}) => {
  if (!sharedAdminSession) await enterAdmin(page);
  await page.emulateMedia({ colorScheme: "dark" });
  const recording = await recordingOf(page);
  await page.goto(`/recordings/${recording.id}?tab=lyrics`);
  await mock(page, recording.id, recording.duration_ms);
  await page.reload();
  const panel = page.getByRole("region", { name: "Lyrics" });
  await expect(panel.getByText("Synced · from LRCLIB lyrics")).toBeVisible();
  const states = await panel
    .locator(".np-lines button")
    .evaluateAll((buttons) =>
      buttons.map((button) => (button as HTMLElement).dataset.state),
    );
  expect(states.length).toBe(lines.length);
  expect(new Set(states)).toEqual(new Set(["next"]));
  await expect(panel.locator("[aria-current]")).toHaveCount(0);
  const inside = await panel.evaluate((element) => {
    const card = element.querySelector(".lyric-panel")!.getBoundingClientRect();
    const scroller = element.querySelector<HTMLElement>(".np-lines")!;
    const box = scroller.getBoundingClientRect();
    return {
      contained: box.bottom <= card.bottom + 1 && box.top >= card.top,
      scrolls: getComputedStyle(scroller).overflowY === "auto",
    };
  });
  expect(inside).toEqual({ contained: true, scrolls: true });
  await panel.getByRole("button", { name: lines[3] }).click({ force: true });
  expect(
    await page.evaluate(
      () =>
        document
          .querySelector("audio[data-hito-player]")
          ?.getAttribute("src") ?? null,
    ),
  ).toBeNull();
  await expect(panel.getByRole("button", { name: "Change" })).toBeVisible();
  await expect(panel.getByRole("radio")).toHaveCount(0);
  await expect(panel.getByRole("button", { name: /^Remove/ })).toHaveCount(0);
  await expect(panel.getByRole("button", { name: "Retry lyrics" })).toHaveCount(
    0,
  );
  await page
    .locator(".track-header")
    .getByRole("button", { name: "Play", exact: true })
    .click();
  const current = panel.locator('[aria-current="true"]');
  await expect(current).toHaveText(lines[0], { timeout: 15_000 });
  await shot(page, "tab-synced");
  await page.getByRole("button", { name: "Pause" }).first().click();
});

test("Track Details lyrics stay static without a matching duration", async ({
  page,
}) => {
  if (!sharedAdminSession) await enterAdmin(page);
  await page.emulateMedia({ colorScheme: "dark" });
  const recording = await recordingOf(page);
  await page.goto(`/recordings/${recording.id}?tab=lyrics`);
  await mock(page, recording.id, 10_000);
  await page.reload();
  const panel = page.getByRole("region", { name: "Lyrics" });
  await expect(
    panel.getByText("Not synced · from LRCLIB lyrics"),
  ).toBeVisible();
  await expect(panel.locator("[aria-current]")).toHaveCount(0);
  await shot(page, "tab-unsynced");
  await panel.getByRole("button", { name: "Change" }).click();
  const dialog = page.getByRole("dialog", { name: "Lyrics for this track" });
  await expect(
    dialog.getByRole("radio", { name: /LRCLIB lyrics/ }),
  ).toBeChecked();
  await shot(page, "tab-change");
});

test("Now playing follows the music with a matching duration", async ({
  page,
}) => {
  if (!sharedAdminSession) await enterAdmin(page);
  await page.emulateMedia({ colorScheme: "dark" });
  const recording = await recordingOf(page);
  await mock(page, recording.id, recording.duration_ms);
  await page.goto("/library?q=Hz");
  await page.getByRole("button", { name: "List view", exact: true }).click();
  await page
    .getByRole("table", { name: "Tracks", exact: true })
    .locator("tbody tr")
    .filter({ hasText: "Fixture 440 Hz" })
    .getByRole("button", { name: "Play Fixture 440 Hz", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Open now playing", exact: true })
    .click();
  const lyrics = page
    .getByRole("dialog", { name: "Now playing" })
    .getByRole("region", { name: "Lyrics" });
  await expect(lyrics.locator('[aria-current="true"]')).toHaveText(lines[0], {
    timeout: 15_000,
  });
  await expect(lyrics.getByText("Synced · from LRCLIB lyrics")).toBeVisible();
  await shot(page, "np-follow");
});
