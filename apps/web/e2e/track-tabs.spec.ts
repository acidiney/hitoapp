import { expect, test, type Page, type Route } from "@playwright/test";
import { enterAdmin, sharedAdminSession } from "./fixtures/admin-session";
import { providerFixture } from "./fixtures/provider";
import { trackShot } from "./fixtures/track-shots";
import type { Schema } from "../src/shared/schema";

const tabNames = ["Overview", "Lyrics", "Sources", "Metadata", "History"];

function tab(page: Page, name: string) {
  return page.getByRole("tab", { name: new RegExp(`^${name}`) });
}

async function selectedTab(page: Page) {
  return page
    .getByRole("tab", { selected: true })
    .evaluate((element) => element.textContent?.replace(/(Edited|\d+)$/, ""));
}

test("a library track walks through every tab with the mouse and the keyboard and keeps its tab on reload", async ({
  page,
}) => {
  if (!sharedAdminSession) await enterAdmin(page);
  const listed = await page.request.get(
    "/api/v1/recordings?q=Fixture%20440%20Hz",
  );
  expect(listed.status()).toBe(200);
  const recording: Schema<"Recording"> | undefined = (
    await listed.json()
  ).items.find((item: Schema<"Recording">) => item.title === "Fixture 440 Hz");
  if (!recording) throw new Error("The scanned fixture library is required");
  const base = `/recordings/${recording.id}`;
  await page.goto(base);
  const header = page.locator(".track-header");
  await expect(page.getByRole("tablist", { name: "Track" })).toBeVisible();
  await expect(header.getByRole("heading", { level: 1 })).toHaveText(
    recording.title,
  );
  expect(await selectedTab(page)).toBe("Overview");
  await expect(
    header.getByRole("button", { name: "Play", exact: true }),
  ).toBeVisible();
  await expect(page.getByRole("tabpanel", { name: "Overview" })).toBeVisible();
  await expect(
    header.getByRole("button", { name: `Like ${recording.title}` }),
  ).toBeVisible();
  await header.getByRole("button", { name: `Like ${recording.title}` }).click();
  await expect(
    header.getByRole("button", { name: `Unlike ${recording.title}` }),
  ).toBeVisible();
  await page.reload();
  await expect(
    header.getByRole("button", { name: `Unlike ${recording.title}` }),
  ).toBeVisible();
  await header
    .getByRole("button", { name: `Unlike ${recording.title}` })
    .click();
  await expect(
    header.getByRole("button", { name: `Like ${recording.title}` }),
  ).toBeVisible();
  await header.getByRole("button", { name: "More actions" }).click();
  await expect(
    header.getByRole("menuitem", { name: "Retry artwork" }),
  ).toBeVisible();
  await page.keyboard.press("Escape");
  await trackShot(page, "01-overview");
  await trackShot(page, "19-overview-light", "desktop-light");
  await trackShot(page, "p1-overview-phone", "phone-dark");

  for (const name of tabNames.slice(1)) {
    await tab(page, name).click();
    await expect(page.getByRole("tabpanel", { name })).toBeVisible();
    await expect(page).toHaveURL(new RegExp(`\\?tab=${name.toLowerCase()}$`));
  }
  await page.reload();
  expect(await selectedTab(page)).toBe("History");
  await trackShot(page, "18-history");

  await tab(page, "Overview").focus();
  await page.keyboard.press("ArrowRight");
  expect(await selectedTab(page)).toBe("Lyrics");
  await page.keyboard.press("End");
  expect(await selectedTab(page)).toBe("History");
  await page.keyboard.press("Home");
  expect(await selectedTab(page)).toBe("Overview");
  await page.locator("body").click({ position: { x: 5, y: 5 } });
  await page.keyboard.press("3");
  expect(await selectedTab(page)).toBe("Sources");
  await trackShot(page, "06-sources-local-ready");

  await page.keyboard.press("4");
  expect(await selectedTab(page)).toBe("Metadata");
  await trackShot(page, "14-metadata-view");
  await page.getByRole("button", { name: "Edit track title" }).click();
  const input = page.getByRole("textbox", { name: "Track title" });
  await expect(input).toBeFocused();
  await page.keyboard.press("2");
  await expect(input).toHaveValue(new RegExp(`${recording.title}2$`));
  await trackShot(page, "15-metadata-edit-title");
  await trackShot(page, "p4-metadata-edit-phone", "phone-dark");
  await page.keyboard.press("Escape");
  await expect(input).toBeHidden();
  await expect(
    page.getByRole("button", { name: "Edit track title" }),
  ).toBeFocused();

  await page.getByRole("button", { name: "Edit track title" }).click();
  await input.fill(`${recording.title} (track tabs)`);
  await page.keyboard.press("Enter");
  await expect(page.getByText("1 personal edit")).toBeVisible();
  await expect(
    page.locator(".track-row").first().getByText("Personal"),
  ).toBeVisible();
  await page.getByRole("button", { name: "Reset all to source" }).click();
  await expect(page.getByText("1 personal edit")).toBeHidden();

  await page.goto(`${base}?tab=sources#find-audio`);
  expect(await selectedTab(page)).toBe("Sources");
  await page.goto(`${base}?tab=nonsense`);
  expect(await selectedTab(page)).toBe("Overview");
  await page.goto(`${base}?review=artist`);
  expect(await selectedTab(page)).toBe("Metadata");
  await expect(
    page.getByRole("heading", { name: "Choose the artist" }),
  ).toBeFocused();
});

async function lyricsFor(
  page: Page,
  recording: string,
  installation: Schema<"PluginInstallation">,
  lyrics: Schema<"LyricsLookup">,
) {
  await page.route("**/api/v1/plugins", async (route: Route) => {
    if (route.request().method() !== "GET") return route.continue();
    const response = await route.fetch();
    const settings: Schema<"PluginSettings"> = await response.json();
    await route.fulfill({
      response,
      json: {
        ...settings,
        installations: settings.installations.map((item) =>
          item.id === installation.id
            ? {
                ...item,
                manifest: {
                  ...item.manifest,
                  capabilities: [...item.manifest.capabilities, "lyrics"],
                },
              }
            : item,
        ),
      },
    });
  });
  await page.route(
    `**/api/v1/recordings/${recording}/lyrics`,
    async (route: Route) => {
      if (route.request().method() !== "POST") return route.continue();
      const body = route.request().postDataJSON();
      await route.fulfill({
        json:
          lyrics.status === "ready"
            ? {
                ...lyrics,
                recording_id: recording,
                observation_id: body.observation_id,
              }
            : lyrics,
      });
    },
  );
}

test("a saved provider track without audio finds audio, reviews the match and shows lyrics", async ({
  page,
  baseURL,
}) => {
  if (!sharedAdminSession) await enterAdmin(page);
  const origin = await providerFixture("Tabs origin");
  const candidate = await providerFixture("Tabs candidate");
  const headers = { "x-hito-request": "1", origin: new URL(baseURL!).origin };
  const installed: Schema<"PluginInstallation">[] = [];
  try {
    for (const [index, fixture] of [origin, candidate].entries()) {
      const offered = await page.request.post("/api/v1/plugins/services", {
        headers,
        data: {
          name: `Tabs service ${index}`,
          base_url: fixture.address,
        } satisfies Schema<"OfferPluginService">,
      });
      expect(offered.status()).toBe(201);
      const service: Schema<"PluginService"> = await offered.json();
      const made = await page.request.post("/api/v1/plugins", {
        headers,
        data: {
          service_id: service.id,
          service_revision: service.revision,
          bearer_token: fixture.token,
        } satisfies Schema<"InstallPlugin">,
      });
      expect(made.status()).toBe(201);
      installed.push(await made.json());
    }
    const adopt = async (id: string) => {
      const adopted = await page.request.post(
        `/api/v1/plugins/${installed[0].id}/recordings`,
        {
          headers,
          data: {
            installation_revision: installed[0].revision,
            provider_recording_id: id,
          } satisfies Schema<"AdoptProviderRecording">,
        },
      );
      expect(adopted.status()).toBe(200);
      return (await adopted.json()) as Schema<"Recording">;
    };
    const recording = await adopt(origin.recording.id);
    const without = await adopt(origin.second.id);
    const ready: Schema<"LyricsLookup"> = {
      status: "ready",
      recording_id: recording.id,
      observation_id: "pending",
      document_digest: "0".repeat(64),
      document: {
        recording: { id: origin.recording.id, revision: null },
        lyrics: {
          id: "lyrics",
          rendition_id: "rendition",
          language: "pt",
          lines: [
            { start_ms: 0, end_ms: 4_000, text: "A maré vem devagar" },
            {
              start_ms: 4_000,
              end_ms: 8_000,
              text: "e leva o que ficou na areia",
            },
            { start_ms: 8_000, end_ms: 12_000, text: "e o silêncio fica" },
          ],
          attribution: {
            creators: ["Nadia Ferreira"],
            provider_label: "LRCLIB",
            content_url: "https://lrclib.example/track",
          },
        },
      },
    };
    await lyricsFor(page, recording.id, installed[0], ready);
    const base = `/recordings/${recording.id}`;
    await page.goto(base);
    const header = page.locator(".track-header");
    await expect(
      header.getByRole("button", { name: "Play", exact: true }),
    ).toBeVisible();
    await expect(header.getByText("Plays from plugin")).toBeVisible();
    await expect(
      page.getByRole("list", { name: "Lyrics preview" }),
    ).toBeVisible();
    await page.goto(`${base}?tab=lyrics`);
    await expect(page.getByRole("list", { name: "Lyric lines" })).toBeVisible();
    await expect(page.getByText("Synced", { exact: true })).toBeVisible();
    await trackShot(page, "03-lyrics-synced");

    await page.unroute(`**/api/v1/recordings/${recording.id}/lyrics`);
    await lyricsFor(page, recording.id, installed[0], {
      ...ready,
      document: {
        ...ready.document,
        lyrics: {
          ...ready.document.lyrics!,
          lines: ready.document.lyrics!.lines.map((line) => ({
            ...line,
            start_ms: 0,
            end_ms: 0,
          })),
        },
      },
    } as Schema<"LyricsLookup">);
    await page.reload();
    await expect(page.getByText("Unsynced", { exact: true })).toBeVisible();
    await trackShot(page, "04-lyrics-unsynced");

    await page.unroute(`**/api/v1/recordings/${recording.id}/lyrics`);
    await lyricsFor(page, recording.id, installed[0], {
      ...ready,
      document: { ...ready.document, lyrics: null },
    } as Schema<"LyricsLookup">);
    await page.reload();
    await expect(
      page.getByText("No lyrics found for this recording."),
    ).toBeVisible();
    await trackShot(page, "05-lyrics-none-choose-provider");

    const removed = await page.request.delete(
      `/api/v1/plugins/${installed[0].id}`,
      { headers, data: { revision: installed[0].revision } },
    );
    expect(removed.status()).toBe(204);
    installed.shift();
    await page.unroute(`**/api/v1/recordings/${recording.id}/lyrics`);
    await page.unroute("**/api/v1/plugins");
    expect(without.id).not.toBe(recording.id);
    await page.goto(`/recordings/${without.id}`);
    await expect(header.getByRole("heading", { level: 1 })).toHaveText(
      origin.second.title,
    );
    await expect(
      header.getByRole("button", { name: "Find audio" }),
    ).toBeVisible();
    await expect(header.getByText("Catalog only")).toBeVisible();
    await trackShot(page, "02-overview-no-audio");
    await header.getByRole("button", { name: "Find audio" }).click();
    expect(await selectedTab(page)).toBe("Sources");
    await expect(page.locator("#find-audio")).toBeFocused();
    await trackShot(page, "10-sources-none-find-audio");
    await trackShot(page, "p2-sources-none-phone", "phone-dark");
    const panel = page.getByRole("region", {
      name: /^(Find audio|Add another source)$/,
    });
    await expect(
      panel.getByRole("group", { name: "Music service" }),
    ).toBeVisible();
    await expect(
      panel.getByRole("searchbox", { name: "Search music" }),
    ).toHaveValue(`${without.title} ${without.artist}`);
    await panel
      .getByRole("searchbox", { name: "Search music" })
      .fill(candidate.recording.title);
    await panel.getByRole("button", { name: "Search", exact: true }).click();
    await panel
      .getByRole("button", { name: "Review match", exact: true })
      .first()
      .click();
    const dialog = page.getByRole("dialog", { name: "Review source match" });
    await expect(dialog).toBeVisible();
    await trackShot(page, "11-sources-review-dialog");
    await trackShot(page, "p3-sources-review-dialog-phone", "phone-dark");
    await dialog.getByRole("button", { name: "Cancel" }).click();
    await expect(dialog).toBeHidden();

    await tab(page, "Metadata").click();
    await trackShot(page, "14-metadata-view-provider");
    await page.getByRole("button", { name: "Change artist" }).click();
    await expect(
      page.getByRole("heading", { name: "Choose the artist" }),
    ).toBeFocused();
    await trackShot(page, "16-metadata-change-artist");
  } finally {
    for (const item of installed)
      await page.request.delete(`/api/v1/plugins/${item.id}`, {
        headers,
        data: { revision: item.revision },
      });
    await origin.close();
    await candidate.close();
  }
});
