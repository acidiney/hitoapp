import { expect, test, type Page } from "@playwright/test";
import { enterAdmin, sharedAdminSession } from "./fixtures/admin-session";
import { trackShot } from "./fixtures/track-shots";
import type { Schema } from "../src/shared/schema";

type Mode = "synced" | "none" | "failed" | "loading";

const provider = {
  installation_id: "11111111-1111-4111-8111-111111111111",
  installation_revision: 2,
  label: "Open lyrics",
  observation_id: "22222222-2222-4222-8222-222222222222",
  provider_recording: { id: "auto-1", revision: null },
} satisfies Schema<"RecordingLyricsProvider">;

async function mockCompatible(page: Page, recording: string, mode: Mode) {
  const requests: string[] = [];
  const base = `**/api/v1/recordings/${recording}/lyrics`;
  await page.route(`${base}/identities`, (route) =>
    route.fulfill({ json: { recording_id: recording, items: [] } }),
  );
  await page.route(`${base}/services`, (route) =>
    route.fulfill({
      json: {
        recording_id: recording,
        items: [
          {
            installation_id: provider.installation_id,
            installation_revision: 2,
            label: "Open lyrics",
          },
        ],
      },
    }),
  );
  await page.route(`${base}/providers`, (route) => {
    requests.push("providers");
    return route.fulfill({
      json: { recording_id: recording, items: [provider] },
    });
  });
  await page.route(base, (route) => {
    if (route.request().method() !== "POST") return route.continue();
    requests.push(`read:${route.request().postData()}`);
    if (mode === "loading") return;
    if (mode === "failed")
      return route.fulfill({ status: 502, json: { error: "down" } });
    return route.fulfill({
      json: {
        status: "ready",
        recording_id: recording,
        observation_id: provider.observation_id,
        document_digest: "0",
        document: {
          recording: provider.provider_recording,
          lyrics:
            mode === "none"
              ? null
              : {
                  id: "l",
                  rendition_id: "r",
                  lines: [
                    { start_ms: 0, end_ms: 4000, text: "Should auld acquaintance be forgot," },
                    { start_ms: 4000, end_ms: 8000, text: "And never brought to mind?" },
                  ],
                  attribution: null,
                },
        },
      } satisfies Schema<"LyricsLookup">,
    });
  });
  for (const suffix of ["candidates", "identities/review", "identities/read"])
    await page.route(`${base}/${suffix}`, (route) => {
      requests.push(`forbidden:${suffix}`);
      return route.abort();
    });
  return requests;
}

for (const mode of ["loading", "synced", "none", "failed"] as const)
  test(`automatic lyric lookup on Track Details: ${mode}`, async ({ page }) => {
    if (!sharedAdminSession) await enterAdmin(page);
    await page.emulateMedia({ colorScheme: "dark" });
    const listed = await page.request.get(
      "/api/v1/recordings?q=Fixture%20440%20Hz",
    );
    const recording: Schema<"Recording"> = (await listed.json()).items.find(
      (item: Schema<"Recording">) => item.title === "Fixture 440 Hz",
    );
    const requests = await mockCompatible(page, recording.id, mode);
    await page.goto(`/recordings/${recording.id}?tab=lyrics`);
    const panel = page.getByRole("region", { name: "Lyrics" });
    const expected = {
      loading: "Looking up lyrics…",
      synced: "Synced · from Open lyrics",
      none: "None found",
      failed: "Couldn't check",
    }[mode];
    await expect(panel.getByText(expected).first()).toBeVisible();
    expect(requests.filter((entry) => entry.startsWith("forbidden"))).toEqual([]);
    await trackShot(page, `auto-${mode}`);
    await trackShot(page, `auto-${mode}-phone`, "phone-dark");
  });
