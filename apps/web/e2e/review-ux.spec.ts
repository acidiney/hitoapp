import { expect, test, type Page } from "@playwright/test";
import { providerFixture } from "./fixtures/provider";
import { enterAdmin, sharedAdminSession } from "./fixtures/admin-session";
import { artistReviewProvider } from "./fixtures/artist-review-provider";
import { shoot } from "./fixtures/ux-shots";
import type { Schema } from "../src/shared/schema";

async function adoptLibrary(page: Page, baseURL: string, prefix: string) {
  const fixture = await providerFixture(prefix, true, false, true);
  const headers = { "x-hito-request": "1", origin: new URL(baseURL).origin };
  await page.goto("/library");
  await expect(
    page.getByRole("heading", { name: "Your library", exact: true }),
  ).toBeVisible();
  const offered = await page.request.post("/api/v1/plugins/services", {
    headers,
    data: {
      name: `${prefix} service`,
      base_url: fixture.address,
    } satisfies Schema<"OfferPluginService">,
  });
  expect(offered.status()).toBe(201);
  const service: Schema<"PluginService"> = await offered.json();
  const installed = await page.request.post("/api/v1/plugins", {
    headers,
    data: {
      service_id: service.id,
      service_revision: service.revision,
      bearer_token: fixture.token,
    } satisfies Schema<"InstallPlugin">,
  });
  expect(installed.status()).toBe(201);
  const connection: Schema<"PluginInstallation"> = await installed.json();
  const ids = [
    fixture.recording.id,
    fixture.second.id,
    ...Array.from(
      { length: 5 },
      (_, index) => `provider/private/catalog-${index}`,
    ),
  ];
  for (const id of ids) {
    const adopted = await page.request.post(
      `/api/v1/plugins/${connection.id}/recordings`,
      {
        headers,
        data: {
          installation_revision: connection.revision,
          provider_recording_id: id,
        } satisfies Schema<"AdoptProviderRecording">,
      },
    );
    expect(adopted.status()).toBe(200);
  }
  return {
    fixture,
    cleanup: async () => {
      await page.request.delete(`/api/v1/plugins/${connection.id}`, {
        headers,
        data: { revision: connection.revision },
      });
      await fixture.close();
    },
  };
}

test("the artists grid loads the next pages by scrolling and keeps what it loaded after going back", async ({
  page,
  baseURL,
}) => {
  const { cleanup } = await adoptLibrary(page, baseURL!, "Scroll");
  const offsets: string[] = [];
  page.on("request", (request) => {
    const url = new URL(request.url());
    if (url.pathname === "/api/v1/artists" && url.searchParams.has("offset"))
      offsets.push(url.searchParams.get("offset")!);
  });
  try {
    const first = await page.request.get("/api/v1/artists?limit=2");
    const total: number = (await first.json()).total;
    expect(total).toBeGreaterThanOrEqual(5);
    await page.goto("/artists?limit=2");
    const credits = page.getByRole("region", { name: "Library credits" });
    const cards = credits.getByRole("link");
    await shoot(page, "artists-grid");
    await expect(cards).toHaveCount(total, { timeout: 20000 });
    expect(offsets[0]).toBe("2");
    await expect(credits.getByRole("button", { name: "Previous" })).toHaveCount(
      0,
    );
    await expect(
      credits.getByRole("button", { name: "Load more" }),
    ).toHaveCount(0);
    await shoot(page, "artists-grid-loaded", { full: true });
    await cards.last().click();
    await expect(page).toHaveURL(/\/artists\/[^/]+/);
    const requests = offsets.length;
    await page.goBack();
    await expect(
      page.getByRole("region", { name: "Library credits" }).getByRole("link"),
    ).toHaveCount(total);
    expect(offsets).toHaveLength(requests);
  } finally {
    await cleanup();
  }
});

test("without a scroll signal the artists grid still loads more with the Load more button", async ({
  page,
  baseURL,
}) => {
  await page.addInitScript(() => {
    Reflect.deleteProperty(window, "IntersectionObserver");
  });
  const { cleanup } = await adoptLibrary(page, baseURL!, "Button");
  try {
    const first = await page.request.get("/api/v1/artists?limit=2");
    const total: number = (await first.json()).total;
    await page.goto("/artists?limit=2");
    const credits = page.getByRole("region", { name: "Library credits" });
    const cards = credits.getByRole("link");
    await expect(cards).toHaveCount(2);
    await expect(
      credits.getByText(`Showing 2 of ${total} artists`),
    ).toBeVisible();
    await credits.getByRole("button", { name: "Load more" }).click();
    await expect(cards).toHaveCount(4);
    await shoot(page, "artists-grid-button", { full: true });
  } finally {
    await cleanup();
  }
});

test("the player bar links the title to its track page and the artist name to an artist page", async ({
  page,
  baseURL,
}) => {
  const { cleanup } = await adoptLibrary(page, baseURL!, "Dock");
  try {
    await page.goto("/library?q=Dock");
    await page
      .getByRole("button", { name: /^Play Dock catalog fixture 1/ })
      .first()
      .click();
    const dock = page.getByRole("contentinfo", { name: "Audio player" });
    const title = dock.getByRole("link", { name: "Dock catalog fixture 1" });
    const artist = dock.getByRole("link", { name: "Dock artist 1" });
    await expect(title).toBeVisible();
    await expect(artist).toBeVisible();
    await shoot(page, "player-bar");
    await expect(title).toHaveAttribute("href", /^\/(albums|recordings)\//);
    await expect(artist).toHaveAttribute(
      "href",
      /^\/(artist-groups|artists|search)/,
    );
    await title.focus();
    await page.keyboard.press("Enter");
    await expect(page).toHaveURL(/\/(albums|recordings)\//);
    await expect(page.getByRole("heading", { level: 1 })).toBeFocused();
    await dock.getByRole("link", { name: "Dock artist 1" }).click();
    await expect(page).toHaveURL(/\/(artist-groups|artists|search)/);
  } finally {
    await cleanup();
  }
});

test("an artist match opens in a dialog beside the saved credit and returns focus when cancelled", async ({
  page,
  baseURL,
}) => {
  test.skip(
    process.env.HITO_ARTIST_REVIEW_E2E !== "1",
    "Requires the opted-in disposable artist provider",
  );
  const query = "Owned namesake";
  if (!sharedAdminSession) await enterAdmin(page);
  const { fixture, cleanup } = await adoptLibrary(page, baseURL!, "Artist");
  const provider = await artistReviewProvider(query);
  const headers = { "x-hito-request": "1", origin: new URL(baseURL!).origin };
  try {
    const offered = await page.request.post("/api/v1/plugins/services", {
      headers,
      data: { name: "Artist catalog", base_url: provider.address },
    });
    expect(offered.status()).toBe(201);
    const service: Schema<"PluginService"> = await offered.json();
    const installed = await page.request.post("/api/v1/plugins", {
      headers,
      data: {
        service_id: service.id,
        service_revision: service.revision,
        bearer_token: provider.token,
      } satisfies Schema<"InstallPlugin">,
    });
    expect(installed.status()).toBe(201);
    const connection: Schema<"PluginInstallation"> = await installed.json();
    const saved = await page.request.put("/api/v1/metadata/settings", {
      headers,
      data: {
        revision: 0,
        enabled: true,
        providers: [{ kind: "http_plugin", installation_id: connection.id }],
      } satisfies Schema<"UpdateMetadataSettings">,
    });
    expect(saved.status()).toBe(200);
    const catalog: Schema<"RecordingPage"> = await (
      await page.request.get("/api/v1/recordings?q=Artist")
    ).json();
    await page.goto(`/recordings/${catalog.items[0].id}?tab=metadata`);
    const artist = page.getByRole("region", { name: "Linked artist" });
    await artist.getByRole("button", { name: "Change artist" }).click();
    await artist.getByRole("searchbox", { name: "Search artists" }).fill(query);
    await artist.getByRole("button", { name: "Search artists" }).click();
    const results = artist.getByRole("list", { name: "Artist search results" });
    await expect(results.getByRole("listitem")).toHaveCount(2);
    await shoot(page, "artist-candidates", { focus: results });
    const open = results
      .getByRole("listitem")
      .nth(1)
      .getByRole("button", { name: "Review artist" });
    await open.click();
    const dialog = page.getByRole("dialog", { name: "Review artist" });
    await expect(dialog).toBeVisible();
    await expect(
      dialog.getByRole("group", { name: "Provider profile" }),
    ).toContainText("Acoustic singer from Luanda");
    await expect(
      dialog.getByRole("group", { name: "Saved track" }),
    ).toContainText(catalog.items[0].artist);
    await shoot(page, "artist-review", { focus: dialog });
    await dialog.getByRole("button", { name: "Cancel" }).click();
    await expect(dialog).toBeHidden();
    await expect(open).toBeFocused();
    await open.click();
    await dialog.getByRole("button", { name: "Use this artist" }).click();
    await expect(dialog).toBeHidden();
    await expect(page.getByText("Artist review saved.")).toBeVisible();
    await page.request.delete(`/api/v1/plugins/${connection.id}`, {
      headers,
      data: { revision: connection.revision },
    });
  } finally {
    await provider.close();
    await cleanup();
    void fixture;
  }
});
