import { expect, test, type Page } from "@playwright/test";
import { enterAdmin, sharedAdminSession } from "./fixtures/admin-session";
import { trackShot } from "./fixtures/track-shots";
import type { Schema } from "../src/shared/schema";

type Mode = "found" | "none" | "unavailable" | "ambiguous" | "loading";

const service = {
  installation_id: "11111111-1111-4111-8111-111111111111",
  installation_revision: 2,
  label: "Open lyrics",
} satisfies Schema<"RecordingLyricService">;

const providerRecording = { id: "auto-1", revision: null };

async function mockAutomatic(
  page: Page,
  recording: Schema<"Recording">,
  mode: Mode,
) {
  const requests: string[] = [];
  const base = `**/api/v1/recordings/${recording.id}/lyrics`;
  await page.route(`${base}/services`, (route) =>
    route.fulfill({ json: { recording_id: recording.id, items: [service] } }),
  );
  await page.route(`${base}/identities`, (route) => {
    if (route.request().method() === "POST") requests.push("retain");
    return route.fulfill({
      json: { recording_id: recording.id, items: [] },
    });
  });
  for (const other of ["candidates", "identities/review", "identities/read"])
    await page.route(`${base}/${other}`, (route) => {
      requests.push(other);
      return route.fulfill({ status: 500, json: { error: "unexpected" } });
    });
  await page.route(`${base}/automatic`, (route) => {
    requests.push(`automatic:${route.request().postData()}`);
    if (mode === "loading") return;
    const status = { none: "not_found", unavailable: "unavailable" }[
      mode as "none" | "unavailable"
    ];
    if (mode === "ambiguous")
      return route.fulfill({ json: { status: "ambiguous" } });
    if (status) return route.fulfill({ json: { status } });
    return route.fulfill({
      json: {
        status: "ready_v2",
        recording_id: recording.id,
        installation_id: service.installation_id,
        installation_revision: service.installation_revision,
        label: service.label,
        provider_recording: providerRecording,
        document_digest: "0",
        document: {
          recording: providerRecording,
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
      } satisfies Schema<"AutomaticLyricsLookup">,
    });
  });
  return requests;
}

for (const mode of [
  "loading",
  "found",
  "none",
  "unavailable",
  "ambiguous",
] as const)
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
    const panel = page.getByRole("region", { name: "Lyrics", exact: true });
    const expected = {
      loading: "Looking up lyrics…",
      found: "Not synced · from Open lyrics",
      none: "None found",
      unavailable: "Couldn't check",
      ambiguous: "Several matches",
    }[mode];
    await expect(panel.getByText(expected).first()).toBeVisible();
    await expect.poll(() => requests).toEqual(["automatic:{}"]);
    if (mode === "none")
      await expect(
        panel.getByRole("button", { name: "Find lyrics" }),
      ).toBeVisible();
    if (mode === "unavailable") {
      await expect(panel.getByText("None found")).toHaveCount(0);
      await expect(panel.getByRole("button", { name: "Retry" })).toBeVisible();
    }
    await trackShot(page, `auto-${mode}`);
    await trackShot(page, `auto-${mode}-phone`, "phone-dark");
  });
