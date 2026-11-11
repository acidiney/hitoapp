import { expect, test, type Page, type Route } from "@playwright/test";
import { enterAdmin, sharedAdminSession } from "./fixtures/admin-session";
import { trackShot } from "./fixtures/track-shots";
import type { Schema } from "../src/shared/schema";

const service = {
  installation_id: "11111111-1111-4111-8111-111111111111",
  installation_revision: 7,
  label: "Open lyrics",
} satisfies Schema<"RecordingLyricService">;

const unavailable = {
  selectable: false,
  selector: {
    kind: "reviewed",
    identity_id: "33333333-3333-4333-8333-333333333333",
    identity_revision: 2,
  },
  installation_id: "44444444-4444-4444-8444-444444444444",
  installation_revision: 1,
  provider_recording: { id: "gone-1", revision: null },
  label: "Retired lyrics",
} satisfies Schema<"RecordingLyricIdentity">;

const created = {
  selectable: true,
  selector: {
    kind: "reviewed",
    identity_id: "55555555-5555-4555-8555-555555555555",
    identity_revision: 1,
  },
  installation_id: service.installation_id,
  installation_revision: service.installation_revision,
  provider_recording: { id: "match-1", revision: null },
  label: service.label,
} satisfies Schema<"RecordingLyricIdentity">;

const found = {
  id: "match-1",
  title: "Matched title",
  artist: "Matched artist",
  duration_ms: 212_000,
  release: { id: "rel-1", title: "Matched album" },
} satisfies Schema<"ProviderRecording">;

async function mockLyrics(page: Page, recording: string) {
  const requests: { method: string; path: string; body: unknown }[] = [];
  let saved = false;
  const base = `**/api/v1/recordings/${recording}/lyrics`;
  const note = (route: Route) => {
    const request = route.request();
    requests.push({
      method: request.method(),
      path: new URL(request.url()).pathname.replace(
        `/api/v1/recordings/${recording}/lyrics`,
        "",
      ),
      body: request.postDataJSON?.() ?? null,
    });
  };
  await page.route(`${base}/services`, (route) => {
    note(route);
    return route.fulfill({
      json: {
        recording_id: recording,
        items: [service],
      } satisfies Schema<"RecordingLyricServices">,
    });
  });
  await page.route(`${base}/identities`, (route) => {
    note(route);
    if (route.request().method() === "POST") {
      saved = true;
      return route.fulfill({ json: created });
    }
    return route.fulfill({
      json: {
        recording_id: recording,
        items: saved ? [unavailable, created] : [unavailable],
      } satisfies Schema<"RecordingLyricIdentities">,
    });
  });
  await page.route(`${base}/candidates`, (route) => {
    note(route);
    return route.fulfill({
      json: { items: [found] } satisfies Schema<"SearchPage">,
    });
  });
  await page.route(`${base}/identities/review`, (route) => {
    note(route);
    return route.fulfill({
      json: {
        candidate: { recording: found, album_title: "Matched album" },
        review_stamp: "opaque-stamp",
      } satisfies Schema<"RecordingLyricReview">,
    });
  });
  await page.route(`${base}/identities/read`, (route) => {
    note(route);
    return route.fulfill({
      json: {
        status: "ready_v2",
        recording_id: recording,
        selector: created.selector,
        provider_recording: created.provider_recording,
        document_digest: "0".repeat(64),
        document: {
          recording: created.provider_recording,
          lyrics: {
            id: "doc",
            rendition_id: "rendition",
            content: {
              kind: "plain",
              lines: [
                "First line",
                "",
                '<img src=x onerror="window.__lyricXss=1">',
              ],
            },
            attribution: {
              provider_label: "Open lyrics",
              content_url: "https://lyrics.example/track/1",
            },
          },
        },
      } satisfies Schema<"SelectedLyricsLookup">,
    });
  });
  return requests;
}

test("track details Lyrics: choose service, search, review, accept, then read, with mocked wire shapes", async ({
  page,
}) => {
  if (!sharedAdminSession) await enterAdmin(page);
  const listed = await page.request.get(
    "/api/v1/recordings?q=Fixture%20440%20Hz",
  );
  const recording: Schema<"Recording"> = (await listed.json()).items.find(
    (item: Schema<"Recording">) => item.title === "Fixture 440 Hz",
  );
  const requests = await mockLyrics(page, recording.id);
  await page.goto(`/recordings/${recording.id}?tab=lyrics`);
  const panel = page.getByRole("region", { name: "Lyrics" });
  await expect(panel.getByRole("radio")).toHaveCount(0);
  await panel.getByRole("button", { name: "Change" }).click();
  const choices = page.getByRole("dialog", { name: "Lyrics for this track" });
  await expect(
    choices.getByRole("radio", { name: /Retired lyrics/ }),
  ).toBeDisabled();
  await expect(
    choices.getByRole("button", { name: "Remove Retired lyrics" }),
  ).toBeEnabled();
  await trackShot(page, "lyr-01-choices");
  await trackShot(page, "lyr-01-choices-phone", "phone-dark");
  expect(requests.map((entry) => entry.path).sort()).toEqual([
    "/identities",
    "/services",
  ]);

  await choices.getByRole("button", { name: "Find lyrics" }).click();
  await expect(
    panel.getByRole("searchbox", { name: "Search text" }),
  ).toHaveValue(`${recording.title} ${recording.artist}`);
  await trackShot(page, "lyr-02-find");
  expect(requests).toHaveLength(2);
  await panel.getByRole("button", { name: "Search", exact: true }).click();
  await panel
    .getByRole("button", { name: "Review lyrics for Matched title" })
    .click();
  const dialog = page.getByRole("dialog", {
    name: "Are these the lyrics you want?",
  });
  await expect(dialog.getByText("Matched album").first()).toBeVisible();
  await trackShot(page, "lyr-03-review");
  await trackShot(page, "lyr-03-review-phone", "phone-dark");
  await dialog.getByRole("button", { name: "Use these lyrics" }).click();
  await expect(dialog).toBeHidden();
  await expect(panel.getByText(/from Open lyrics/)).toBeVisible();
  const retain = requests.find(
    (entry) => entry.method === "POST" && entry.path === "/identities",
  );
  expect(retain?.body).toEqual({
    candidate: {
      installation_id: service.installation_id,
      installation_revision: service.installation_revision,
      provider_recording_id: "match-1",
    },
    review_stamp: "opaque-stamp",
  });
  expect(requests.some((entry) => entry.path === "/identities/read")).toBe(
    false,
  );
  await panel.getByRole("button", { name: "Show lyrics" }).click();
  await expect(panel.getByText("First line")).toBeVisible();
  await expect(
    panel.getByText('<img src=x onerror="window.__lyricXss=1">'),
  ).toBeVisible();
  expect(await page.evaluate(() => "__lyricXss" in window)).toBe(false);
  await expect(
    panel.getByRole("link", { name: "Open lyrics" }),
  ).toHaveAttribute("href", "https://lyrics.example/track/1");
  await trackShot(page, "lyr-04-read");
  await trackShot(page, "lyr-04-read-phone", "phone-dark");
});
