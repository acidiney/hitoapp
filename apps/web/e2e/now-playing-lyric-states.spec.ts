import { expect, test, type Page } from "@playwright/test";
import { enterAdmin, sharedAdminSession } from "./fixtures/admin-session";
import type { Schema } from "../src/shared/schema";

type Reading = Schema<"SelectedLyricsLookup"> | "none" | "unavailable" | "slow";

async function mockNowPlaying(page: Page, reading: Reading) {
  await page.route(
    /\/api\/v1\/recordings\/[^/]+\/lyrics\/(identities|services|identities\/read)$/,
    async (route) => {
      const url = new URL(route.request().url());
      const recording = url.pathname.split("/")[4];
      const identity: Schema<"RecordingLyricIdentity"> = {
        selectable: true,
        selector: { kind: "observation", observation_id: "obs-np" },
        installation_id: "11111111-1111-4111-8111-111111111111",
        installation_revision: 1,
        label: "Open lyrics",
        provider_recording: { id: "np-1", revision: null },
      };
      if (url.pathname.endsWith("/services"))
        return route.fulfill({
          json: { recording_id: recording, items: [] },
        });
      if (url.pathname.endsWith("/identities/read")) {
        if (reading === "slow") return;
        if (reading === "unavailable")
          return route.fulfill({ json: { status: "unavailable" } });
        if (reading === "none")
          return route.fulfill({
            json: {
              status: "ready_v2",
              recording_id: recording,
              selector: identity.selector,
              provider_recording: identity.provider_recording,
              document_digest: "0",
              document: {
                recording: identity.provider_recording,
                lyrics: null,
              },
            } satisfies Schema<"SelectedLyricsLookup">,
          });
        return route.fulfill({
          json: { ...reading, recording_id: recording },
        });
      }
      return route.fulfill({
        json: { recording_id: recording, items: [identity] },
      });
    },
  );
}

const ref = { id: "np-1", revision: null };
const selector = { kind: "observation", observation_id: "obs-np" } as const;
const synced: Reading = {
  status: "ready_v1",
  recording_id: "x",
  selector,
  provider_recording: ref,
  document_digest: "0",
  document: {
    recording: ref,
    lyrics: {
      id: "l",
      rendition_id: "r",
      lines: [
        {
          start_ms: 0,
          end_ms: 4000,
          text: "Should auld acquaintance be forgot,",
        },
        { start_ms: 4000, end_ms: 8000, text: "And never brought to mind?" },
        { start_ms: 8000, end_ms: 12000, text: "And auld lang syne!" },
      ],
      attribution: null,
    },
  },
};
const plain: Reading = {
  status: "ready_v2",
  recording_id: "x",
  selector,
  provider_recording: ref,
  document_digest: "0",
  document: {
    recording: ref,
    lyrics: {
      id: "l",
      rendition_id: "r",
      content: {
        kind: "plain",
        lines: [
          "Should auld acquaintance be forgot,",
          "And never brought to mind?",
          "",
          "For auld lang syne, my dear,",
          "For auld lang syne.",
        ],
      },
    },
  },
};

const states: [string, Reading][] = [
  ["synced", synced],
  ["unsynced", plain],
  ["none", "none"],
  ["failed", "unavailable"],
  ["loading", "slow"],
];

for (const [name, reading] of states)
  test(`now playing lyrics state: ${name}`, async ({ page }) => {
    if (!sharedAdminSession) await enterAdmin(page);
    await mockNowPlaying(page, reading);
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
    const dialog = page.getByRole("dialog", { name: "Now playing" });
    const lyrics = dialog.getByRole("region", { name: "Lyrics" });
    if (name === "synced")
      await expect(
        lyrics.getByRole("button", { name: /Should auld/ }),
      ).toBeVisible();
    if (name === "unsynced")
      await expect(lyrics.getByText("For auld lang syne.")).toBeVisible();
    if (name === "none")
      await expect(lyrics.getByText("None found")).toBeVisible();
    if (name === "failed")
      await expect(lyrics.getByText("Couldn't check")).toBeVisible();
    if (name === "loading")
      await expect(
        lyrics.getByRole("region", { name: "Looking up lyrics…" }),
      ).toBeVisible();
    const directory = process.env.TRACK_SHOTS;
    if (!directory) return;
    await page.waitForTimeout(600);
    await page.screenshot({ path: `${directory}/np-${name}-desktop.png` });
    await page.setViewportSize({ width: 390, height: 844 });
    await page.waitForTimeout(600);
    await page.screenshot({ path: `${directory}/np-${name}-phone.png` });
  });
