import { expect, test, type Page } from "@playwright/test";
import { enterAdmin, sharedAdminSession } from "./fixtures/admin-session";
import { connectFixtureFolders } from "./fixtures/music-folders";
import { stubTrackPage } from "./fixtures/track-fidelity";
import type { Schema } from "../src/shared/schema";

async function libraryRecording(page: Page) {
  let id = "";
  await expect
    .poll(
      async () => {
        const listed = await page.request.get(
          "/api/v1/recordings?q=Fixture%20440%20Hz",
        );
        if (listed.status() !== 200) return "";
        const recording: Schema<"Recording"> | undefined = (
          await listed.json()
        ).items.find(
          (item: Schema<"Recording">) => item.title === "Fixture 440 Hz",
        );
        id = recording?.id ?? "";
        return id;
      },
      { message: "The scanned fixture library is required", timeout: 60_000 },
    )
    .not.toBe("");
  return id;
}

async function shot(page: Page, name: string) {
  const directory = process.env.SAME_RECORDING_SHOTS;
  if (!directory) return;
  const width = page.viewportSize()?.width ?? 0;
  await page.waitForTimeout(400);
  await page.screenshot({
    path: `${directory}/${name}-${width < 768 ? "phone" : "desktop"}.png`,
    animations: "disabled",
  });
}

function publicSource(
  recording: string,
  id: string,
  service: string,
  label: string,
): Schema<"Source"> {
  return {
    id,
    recording_id: recording,
    provider: "plugin",
    label,
    provider_label: service,
    community_public: true,
    device_id: null,
    ready: true,
    can_prepare: false,
    match_kind: "verified",
  };
}

for (const viewport of [
  { width: 1440, height: 900 },
  { width: 390, height: 844 },
]) {
  test(`a listener suggests two public sources as the same recording at ${viewport.width}px`, async ({
    page,
  }) => {
    test.setTimeout(120_000);
    await page.setViewportSize(viewport);
    if (!sharedAdminSession) await enterAdmin(page);
    await page.emulateMedia({ colorScheme: "dark" });
    await connectFixtureFolders(
      page.request,
      process.env.HITO_TEST_URL ?? "http://localhost:8080",
    );
    const id = await libraryRecording(page);
    await stubTrackPage(page, id, { audio: "spotify" });
    const youtube = publicSource(
      id,
      "9f0b6c1e-2a4d-4e8f-9b1c-3d5e7f9a1b2c",
      "YouTube",
      "Official audio",
    );
    const jamendo = publicSource(
      id,
      "1a2b3c4d-5e6f-4a7b-8c9d-0e1f2a3b4c5d",
      "Jamendo",
      "Album track",
    );
    let listed = [youtube, jamendo].map((source) => ({
      ...source,
      community_public: false,
    }));
    await page.route(`**/api/v1/recordings/${id}/source-choice`, (route) =>
      route.fulfill({
        json: {
          mode: "automatic",
          recording_id: id,
          revision: 3,
          sources: listed.map((source, index) => ({
            source,
            selected: index === 0,
            selectable: true,
          })),
        } satisfies Schema<"SourceChoice">,
      }),
    );
    let consent = false;
    await page.route("**/api/v1/community/preferences", (route) =>
      route.fulfill({
        json: {
          revision: 1,
          playback_reporting: false,
          metadata_corrections: consent,
        } satisfies Schema<"CommunityPreferences">,
      }),
    );
    const sent: { source: string; body: Schema<"SameRecording"> }[] = [];
    await page.route("**/api/v1/sources/*/same-recording", (route) => {
      const source = new URL(route.request().url()).pathname.split("/")[4];
      sent.push({ source, body: route.request().postDataJSON() });
      return route.fulfill({ status: 202, json: "queued" });
    });

    await page.goto(`/recordings/${id}?tab=sources`);
    const sources = page.getByRole("region", { name: "Playback source" });
    await expect(sources).toBeVisible();
    await expect(
      sources.getByRole("button", { name: /^More for/ }),
    ).toHaveCount(0);
    await shot(page, "same-recording-before");

    listed = [youtube, jamendo];
    await page.reload();
    await expect(sources).toBeVisible();
    await expect(
      sources.getByRole("link", { name: "Community settings" }),
    ).toHaveAttribute("href", "/settings?section=privacy");
    await shot(page, "same-recording-consent-off");

    consent = true;
    await page.reload();
    await expect(sources).toBeVisible();
    await sources.getByRole("button", { name: "More for YouTube" }).click();
    await page
      .getByRole("menuitem", { name: "Suggest as same recording" })
      .click();
    const dialog = page.getByRole("dialog", {
      name: "Suggest as same recording",
    });
    await expect(dialog).toContainText(
      "The suggestion goes to the shared community service for review.",
    );
    await shot(page, "same-recording-dialog");
    await dialog.getByRole("button", { name: "Send suggestion" }).click();
    await expect(sources.getByText("Suggestion sent for review")).toBeVisible();
    await shot(page, "same-recording-sent");
    expect(sent).toHaveLength(1);
    expect(sent[0].source).toBe(youtube.id);
    expect(sent[0].body.other_source_id).toBe(jamendo.id);
  });
}
