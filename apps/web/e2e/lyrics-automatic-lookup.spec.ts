import { expect, test, type Page } from "@playwright/test";
import { enterAdmin, sharedAdminSession } from "./fixtures/admin-session";
import { trackShot } from "./fixtures/track-shots";
import type { Schema } from "../src/shared/schema";

type Mode = "found" | "none" | "failed" | "loading";

const service = {
  installation_id: "11111111-1111-4111-8111-111111111111",
  installation_revision: 2,
  label: "Open lyrics",
} satisfies Schema<"RecordingLyricService">;

const saved = {
  selectable: true,
  selector: {
    kind: "reviewed",
    identity_id: "44444444-4444-4444-8444-444444444444",
    identity_revision: 1,
  },
  installation_id: service.installation_id,
  installation_revision: service.installation_revision,
  provider_recording: { id: "auto-1", revision: null },
  label: service.label,
} satisfies Schema<"RecordingLyricIdentity">;

async function mockAutomatic(
  page: Page,
  recording: Schema<"Recording">,
  mode: Mode,
) {
  const requests: string[] = [];
  let retained = false;
  const base = `**/api/v1/recordings/${recording.id}/lyrics`;
  const match = {
    id: "auto-1",
    title:
      mode === "none" ? `${recording.title} Remix Part 2` : recording.title,
    artist: recording.artist,
    duration_ms: recording.duration_ms,
  } satisfies Schema<"ProviderRecording">;
  await page.route(`${base}/services`, (route) =>
    route.fulfill({ json: { recording_id: recording.id, items: [service] } }),
  );
  await page.route(`${base}/identities`, (route) => {
    if (route.request().method() === "POST") {
      requests.push("retain");
      retained = true;
      return route.fulfill({ json: saved });
    }
    return route.fulfill({
      json: { recording_id: recording.id, items: retained ? [saved] : [] },
    });
  });
  await page.route(`${base}/candidates`, (route) => {
    requests.push(`candidates:${route.request().postData()}`);
    if (mode === "loading") return;
    if (mode === "failed")
      return route.fulfill({ status: 502, json: { error: "down" } });
    return route.fulfill({ json: { items: [match] } });
  });
  await page.route(`${base}/identities/review`, (route) => {
    requests.push("review");
    return route.fulfill({
      json: {
        candidate: { recording: match },
        review_stamp: "auto-stamp",
      } satisfies Schema<"RecordingLyricReview">,
    });
  });
  await page.route(`${base}/identities/read`, (route) => {
    requests.push("read");
    return route.fulfill({
      json: {
        status: "ready_v2",
        recording_id: recording.id,
        selector: saved.selector,
        provider_recording: saved.provider_recording,
        document_digest: "0",
        document: {
          recording: saved.provider_recording,
          lyrics: {
            id: "l",
            rendition_id: "r",
            content: {
              kind: "plain",
              lines: [
                "Should auld acquaintance be forgot,",
                "And never brought to mind?",
              ],
            },
          },
        },
      } satisfies Schema<"SelectedLyricsLookup">,
    });
  });
  return requests;
}

for (const mode of ["loading", "found", "none", "failed"] as const)
  test(`automatic lyric lookup on Track Details: ${mode}`, async ({ page }) => {
    if (!sharedAdminSession) await enterAdmin(page);
    await page.emulateMedia({ colorScheme: "dark" });
    const listed = await page.request.get(
      "/api/v1/recordings?q=Fixture%20440%20Hz",
    );
    const recording: Schema<"Recording"> = (await listed.json()).items.find(
      (item: Schema<"Recording">) => item.title === "Fixture 440 Hz",
    );
    const requests = await mockAutomatic(page, recording, mode);
    await page.goto(`/recordings/${recording.id}?tab=lyrics`);
    const panel = page.getByRole("region", { name: "Lyrics" });
    const expected = {
      loading: "Looking up lyrics…",
      found: "Not synced · from Open lyrics",
      none: "None found",
      failed: "Couldn't check",
    }[mode];
    await expect(panel.getByText(expected).first()).toBeVisible();
    if (mode === "found")
      expect(requests).toEqual([
        expect.stringContaining('"query":"Fixture 440 Hz'),
        "review",
        "retain",
        "read",
      ]);
    if (mode === "none") {
      expect(requests).toHaveLength(1);
      await expect(
        panel.getByRole("button", { name: "Find lyrics" }),
      ).toBeVisible();
    }
    await trackShot(page, `auto-${mode}`);
    await trackShot(page, `auto-${mode}-phone`, "phone-dark");
  });
