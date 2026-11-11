import { writeFile } from "node:fs/promises";
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
  provider_recording: { id: "m-1", revision: null },
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

type Mode = "synced" | "unsynced" | "loading" | "none" | "failed";

async function mock(
  page: Page,
  recording: string,
  mode: Mode,
  durationMs: number,
) {
  const base = `**/api/v1/recordings/${recording}/lyrics`;
  await page.route(`${base}/identities`, (route) =>
    route.fulfill({ json: { recording_id: recording, items: [identity] } }),
  );
  await page.route(`${base}/services`, (route) =>
    route.fulfill({ json: { recording_id: recording, items: [] } }),
  );
  await page.route(`${base}/identities/read`, (route) => {
    if (mode === "loading") return;
    if (mode === "failed")
      return route.fulfill({ status: 502, json: { error: "down" } });
    const ready = (lyricsValue: unknown) =>
      route.fulfill({
        json: {
          status: "ready_v2",
          recording_id: recording,
          selector: identity.selector,
          provider_recording: identity.provider_recording,
          document_digest: "0",
          document: {
            recording: identity.provider_recording,
            lyrics: lyricsValue,
          },
        },
      });
    if (mode === "none") return ready(null);
    return ready({
      id: "doc",
      rendition_id: "r",
      duration_ms: mode === "unsynced" ? 10_000 : durationMs,
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
    });
  });
}

const measure = `(() => {
  const props = (el) => {
    if (!el) return null;
    const r = el.getBoundingClientRect();
    const c = getComputedStyle(el);
    return { x: +r.x.toFixed(1), y: +r.y.toFixed(1), w: +r.width.toFixed(1), h: +r.height.toFixed(1),
      font: c.fontFamily.split(',')[0].replace(/"/g, '').replace(' Variable', ''),
      size: c.fontSize, weight: c.fontWeight, lh: c.lineHeight, ls: c.letterSpacing,
      color: c.color, bg: c.backgroundColor, rad: c.borderTopLeftRadius, pad: c.padding,
      gap: c.rowGap, bw: c.borderTopWidth + ' ' + c.borderTopStyle + ' ' + c.borderTopColor, shadow: c.boxShadow };
  };
  const q = (s, root = document) => root.querySelector(s);
  const panel = q('.lyric-panel') || q('.lyric-card');
  const out = { panel: props(panel) };
  if (!panel) return out;
  out.header = props(q('.lyric-source > span', panel));
  out.change = props(q('.lyric-source .lyric-action', panel));
  out.linesBox = props(q('.np-lines', panel));
  out.linePast = props(q('.np-lines [data-state="past"]', panel));
  out.lineCurrent = props(q('.np-lines [aria-current]', panel));
  out.lineNext = props(q('.np-lines [data-state="next"]', panel));
  out.rejoin = props(q('.np-rejoin button', panel));
  out.rejoinNote = props(q('.np-rejoin span', panel));
  const lis = panel.querySelectorAll('.track-lyrics-body ol li');
  out.line = props(lis[0]);
  out.label = props(q('.lyric-state', panel));
  out.dot = props(q('.source-status-dot', panel));
  out.title = props(q('.lyric-card-title', panel));
  out.bar1 = props(panel.querySelectorAll('.lyric-skeleton span')[0]);
  out.bar3 = props(panel.querySelectorAll('.lyric-skeleton span')[2]);
  out.note = props(q('.lyric-card-note', panel));
  const buttons = panel.querySelectorAll('.lyric-card-actions > *');
  out.action1 = props(buttons[0]);
  out.action2 = props(buttons[1]);
  return out;
})()`;

async function recordingOf(page: Page) {
  const listed = await page.request.get(
    "/api/v1/recordings?q=Fixture%20440%20Hz",
  );
  return (await listed.json()).items.find(
    (item: Schema<"Recording">) => item.title === "Fixture 440 Hz",
  ) as Schema<"Recording">;
}

for (const size of [
  { name: "desktop", width: 1440, height: 900 },
  { name: "phone", width: 390, height: 844 },
])
  for (const mode of [
    "synced",
    "unsynced",
    "loading",
    "none",
    "failed",
  ] as const)
    test(`measure Track Details lyrics ${mode} ${size.name}`, async ({
      page,
    }) => {
      if (!sharedAdminSession) await enterAdmin(page);
      await page.emulateMedia({ colorScheme: "dark" });
      await page.setViewportSize({ width: size.width, height: size.height });
      const recording = await recordingOf(page);
      await mock(page, recording.id, mode, recording.duration_ms);
      await page.goto(`/recordings/${recording.id}?tab=lyrics`);
      const panel = page.getByRole("region", { name: "Lyrics", exact: true });
      if (mode === "synced") {
        await page
          .locator(".track-header")
          .getByRole("button", { name: "Play", exact: true })
          .click();
        await expect(panel.locator('[aria-current="true"]')).toBeVisible({
          timeout: 15_000,
        });
        await panel.getByRole("button", { name: lines[3] }).first().click();
        await expect(panel.locator('[aria-current="true"]')).toHaveText(
          lines[3],
        );
        await page.getByRole("button", { name: "Pause" }).first().click();
        await panel.locator(".np-lines").dispatchEvent("wheel");
        await expect(
          panel.getByRole("button", { name: "Back to current line" }),
        ).toBeVisible();
      }
      if (mode === "loading")
        await expect(
          panel.getByText("Looking up lyrics…").first(),
        ).toBeVisible();
      if (mode === "unsynced")
        await expect(panel.getByText(/^Not synced/)).toBeVisible();
      if (mode === "none")
        await expect(panel.getByText("None found").first()).toBeVisible();
      if (mode === "failed")
        await expect(panel.getByText("Couldn't check").first()).toBeVisible();
      await page.waitForTimeout(500);
      const result = await page.evaluate(measure);
      const directory = process.env.TRACK_SHOTS ?? "/tmp";
      await writeFile(
        `${directory}/measure-tab-${mode}-${size.name}.json`,
        JSON.stringify(result, null, 1),
      );
      await page.screenshot({
        path: `${directory}/ms-tab-${mode}-${size.name}.png`,
      });
    });

for (const size of [
  { name: "desktop", width: 1440, height: 900 },
  { name: "phone", width: 390, height: 844 },
])
  test(`measure Now playing lyrics ${size.name}`, async ({ page }) => {
    if (!sharedAdminSession) await enterAdmin(page);
    await page.emulateMedia({ colorScheme: "dark" });
    await page.setViewportSize({ width: size.width, height: size.height });
    const recording = await recordingOf(page);
    await mock(page, recording.id, "synced", recording.duration_ms);
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
    await lyrics.getByRole("button", { name: lines[3] }).first().click();
    await expect(lyrics.locator('[aria-current="true"]')).toHaveText(lines[3]);
    await lyrics.locator(".np-lines").dispatchEvent("wheel");
    await page.waitForTimeout(600);
    const result = await page.evaluate(`(() => {
      const props = (el) => { if (!el) return null; const r = el.getBoundingClientRect(); const c = getComputedStyle(el);
        return { x: +r.x.toFixed(1), y: +r.y.toFixed(1), w: +r.width.toFixed(1), h: +r.height.toFixed(1),
          font: c.fontFamily.split(',')[0].replace(/"/g, '').replace(' Variable', ''), size: c.fontSize, weight: c.fontWeight,
          lh: c.lineHeight, ls: c.letterSpacing, color: c.color, bg: c.backgroundColor, rad: c.borderTopLeftRadius,
          pad: c.padding, gap: c.rowGap, shadow: c.boxShadow }; };
      const p = document.querySelector('.np-lyrics');
      const q = (s) => p && p.querySelector(s);
      return { panel: props(p), linesBox: props(q('.np-lines')), linePast: props(q('[data-state="past"]')),
        lineCurrent: props(q('[aria-current]')), lineNext: props(q('[data-state="next"]')),
        rejoin: props(q('.np-rejoin button')), foot: props(q('.np-lyric-foot')), footText: props(q('.np-lyric-foot > span')),
        footLink: props(q('.np-lyric-foot a')), footChange: props(q('.np-lyric-foot button')) };
    })()`);
    const directory = process.env.TRACK_SHOTS ?? "/tmp";
    await writeFile(
      `${directory}/measure-np-${size.name}.json`,
      JSON.stringify(result, null, 1),
    );
    await page.screenshot({ path: `${directory}/ms-np-${size.name}.png` });
  });
