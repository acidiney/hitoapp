import { expect, test, type Page } from "@playwright/test";
import { providerFixture } from "./fixtures/provider";
import type { Schema } from "../src/shared/schema";
import { openLibrarySection } from "./fixtures/library-sections";

async function signIn(page: Page, username: string, password: string) {
  await page.goto("/discover");
  await page.getByLabel("Username", { exact: true }).fill(username);
  await page.getByLabel("Password", { exact: true }).fill(password);
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Your library", exact: true }),
  ).toBeVisible();
}

test("provider discovery saves, pages and plays privately with stale selection protection", async ({
  page,
  browser,
  baseURL,
}) => {
  const fixture = await providerFixture("Discovery", true);
  const headers = { "x-hito-request": "1", origin: new URL(baseURL!).origin };
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  let connection: Schema<"PluginInstallation"> | null = null;
  try {
    await page.goto("/discover");
    await page
      .getByRole("link", { name: "Connect a music service", exact: true })
      .click();
    await expect(
      page.getByRole("heading", { name: "Your plugins", exact: true }),
    ).toBeVisible();
    const offered = await page.request.post("/api/v1/plugins/services", {
      headers,
      data: {
        name: "Discovery collection",
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
    connection = await installed.json();
    await page.goto("/discover");
    const services = page.getByRole("group", { name: "Music service" });
    await expect(
      services.getByRole("button", { name: /Discovery collection/ }),
    ).toHaveAttribute("aria-pressed", "true");
    await expect(services.getByRole("button")).toHaveCount(1);
    const search = page.getByRole("searchbox", { name: "Search music" });
    await expect(page.getByRole("searchbox")).toHaveCount(1);
    await search.fill("Discovery");
    await page.getByRole("button", { name: "Search", exact: true }).click();
    await expect(
      page.getByRole("button", {
        name: `Play ${fixture.recording.title}`,
        exact: true,
      }),
    ).toBeEnabled();
    const listed = await page.request.get("/api/v1/recordings?q=Discovery");
    expect((await listed.json()).total).toBe(0);
    await page.screenshot({
      path: "/workspace/test-results/discovery-desktop.png",
    });
    await page.setViewportSize({ width: 390, height: 844 });
    const searchButton = page.getByRole("button", {
      name: "Search",
      exact: true,
    });
    await searchButton.evaluate((element) =>
      element.scrollIntoView({ block: "center" }),
    );
    const searchBounds = await searchButton.boundingBox();
    const playerBounds = await page
      .getByRole("contentinfo", { name: "Audio player" })
      .boundingBox();
    expect(searchBounds!.y + searchBounds!.height).toBeLessThanOrEqual(
      playerBounds!.y,
    );
    await page.screenshot({
      path: "/workspace/test-results/discovery-mobile-form.png",
    });
    const last = page
      .locator(".provider-result")
      .last()
      .getByRole("button", { name: /Add .* to library/ });
    await last.evaluate((element) =>
      element.scrollIntoView({ block: "center" }),
    );
    const control = await last.boundingBox();
    const dock = await page
      .getByRole("contentinfo", { name: "Audio player" })
      .boundingBox();
    expect(control!.y + control!.height).toBeLessThanOrEqual(dock!.y);
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth > innerWidth,
      ),
    ).toBe(false);
    await page.screenshot({
      path: "/workspace/test-results/discovery-mobile.png",
    });
    await page.setViewportSize({ width: 1440, height: 1000 });
    const pager = page.getByRole("navigation", { name: "Search result pages" });
    await pager.getByRole("button", { name: "Next", exact: true }).click();
    await expect(
      page.getByRole("button", {
        name: `Play ${fixture.second.title}`,
        exact: true,
      }),
    ).toBeVisible();
    await expect(
      page.getByRole("heading", { name: "Search results", exact: true }),
    ).toBeFocused();
    const add = page.getByRole("button", {
      name: `Add ${fixture.second.title} to library`,
      exact: true,
    });
    await add.focus();
    await add.press("Enter");
    const saved = page.getByRole("link", {
      name: `View ${fixture.second.title} in library`,
      exact: true,
    });
    await expect(saved).toBeVisible();
    await expect(saved).toBeFocused();
    expect(fixture.requests).toHaveLength(0);
    await pager.getByRole("button", { name: "Previous", exact: true }).click();
    await pager.getByRole("button", { name: "Next", exact: true }).click();
    await expect(saved).toBeVisible();
    await expect(add).toHaveCount(0);
    await pager.getByRole("button", { name: "Previous", exact: true }).click();
    const refreshed = await page.request.post(
      `/api/v1/plugins/${connection!.id}/refresh`,
      {
        headers,
        data: {
          revision: connection!.revision,
        } satisfies Schema<"RefreshPlugin">,
      },
    );
    expect(refreshed.status()).toBe(200);
    connection = (await refreshed.json()).installation;
    await page.getByRole("button", { name: "Search", exact: true }).click();
    await expect(page.getByRole("alert")).toContainText("changed");
    await page
      .getByRole("link", { name: "Refresh services", exact: true })
      .click();
    await expect(search).toHaveValue("Discovery");
    await page.getByRole("button", { name: "Search", exact: true }).click();
    await expect(
      page.getByRole("button", {
        name: `Play ${fixture.recording.title}`,
        exact: true,
      }),
    ).toBeEnabled();
    const changedAgain = await page.request.post(
      `/api/v1/plugins/${connection!.id}/refresh`,
      {
        headers,
        data: {
          revision: connection!.revision,
        } satisfies Schema<"RefreshPlugin">,
      },
    );
    expect(changedAgain.status()).toBe(200);
    connection = (await changedAgain.json()).installation;
    await page
      .getByRole("button", {
        name: `Play ${fixture.recording.title}`,
        exact: true,
      })
      .click();
    await expect(page.getByRole("alert")).toContainText("changed");
    await page
      .getByRole("link", { name: "Refresh services", exact: true })
      .click();
    await expect(search).toHaveValue("Discovery");
    await page.getByRole("button", { name: "Search", exact: true }).click();
    await page
      .getByRole("button", {
        name: `Play ${fixture.recording.title}`,
        exact: true,
      })
      .click();
    const audio = page.locator("audio[data-hito-player]");
    await expect
      .poll(() =>
        audio.evaluate((element: HTMLAudioElement) => element.currentTime),
      )
      .toBeGreaterThan(0.2);
    const playing = await audio.evaluate(
      (element: HTMLAudioElement) => element.src,
    );
    expect(new URL(playing).pathname).toMatch(
      /^\/api\/v1\/sources\/[a-f0-9-]+\/audio$/,
    );
    await pager.getByRole("button", { name: "Next", exact: true }).click();
    const gate = fixture.holdDetails(fixture.second.id);
    await page
      .getByRole("button", {
        name: `Play ${fixture.second.title}`,
        exact: true,
      })
      .click();
    await expect.poll(() => gate.entered).toBe(true);
    await page.getByRole("button", { name: "Next track", exact: true }).click();
    gate.release();
    await expect(
      page.getByRole("link", {
        name: `View ${fixture.second.title} in library`,
        exact: true,
      }),
    ).toBeVisible();
    await expect
      .poll(() =>
        audio.evaluate((element: HTMLAudioElement) =>
          element.getAttribute("src"),
        ),
      )
      .toBeNull();
    await page
      .getByRole("button", {
        name: `Play ${fixture.second.title}`,
        exact: true,
      })
      .click();
    await expect
      .poll(() =>
        audio.evaluate((element: HTMLAudioElement) => element.currentTime),
      )
      .toBeGreaterThan(0.2);
    const seek = page.getByRole("slider", { name: "Seek", exact: true });
    await seek.focus();
    await seek.press("PageUp");
    await expect
      .poll(() =>
        audio.evaluate((element: HTMLAudioElement) => element.currentTime),
      )
      .toBeGreaterThan(5);
    const before = await audio.evaluate(
      (element: HTMLAudioElement) => element.currentTime,
    );
    const source = await audio.evaluate(
      (element: HTMLAudioElement) => element.src,
    );
    await openLibrarySection(page, "History");
    await expect
      .poll(() =>
        audio.evaluate((element: HTMLAudioElement) => element.currentTime),
      )
      .toBeGreaterThan(before);
    expect(
      await audio.evaluate((element: HTMLAudioElement) => element.src),
    ).toBe(source);
    const owned = await (
      await page.request.get("/api/v1/recordings?q=Discovery")
    ).json();
    const artistIndex = await (
      await page.request.get(
        `/api/v1/artists?library=${encodeURIComponent(owned.items[0].library_id)}`,
      )
    ).json();
    const discoveredArtists = (
      artistIndex.items as { id: string; name: string }[]
    ).filter((artist) => artist.name === fixture.recording.artist);
    expect(discoveredArtists).toHaveLength(1);
    const artistId = discoveredArtists[0].id;
    await openLibrarySection(page, "Artists");
    await page.locator(`a[href="/artists/${artistId}"]`).click();
    await expect(
      page.getByRole("heading", {
        name: fixture.recording.artist,
        exact: true,
        level: 1,
      }),
    ).toBeVisible();
    await expect(
      page.getByRole("heading", { name: "Tracks", exact: true }),
    ).toBeVisible();
    await expect(
      page.getByRole("heading", { name: "Albums & appearances", exact: true }),
    ).toHaveCount(0);
    await expect(
      page.getByRole("button", {
        name: `Play ${fixture.second.title}`,
        exact: true,
      }),
    ).toBeEnabled();
    expect(
      await audio.evaluate((element: HTMLAudioElement) => element.src),
    ).toBe(source);
    await page.screenshot({
      path: "/workspace/test-results/provider-artist-desktop.png",
    });
    await page.setViewportSize({ width: 390, height: 844 });
    const artistPlay = page.getByRole("button", {
      name: "Play tracks",
      exact: true,
    });
    await artistPlay.evaluate((element) =>
      element.scrollIntoView({ block: "center" }),
    );
    const artistControl = await artistPlay.boundingBox();
    const artistDock = await page
      .getByRole("contentinfo", { name: "Audio player" })
      .boundingBox();
    expect(artistControl!.y + artistControl!.height).toBeLessThanOrEqual(
      artistDock!.y,
    );
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth > innerWidth,
      ),
    ).toBe(false);
    await page.screenshot({
      path: "/workspace/test-results/provider-artist-mobile.png",
    });
    await page.setViewportSize({ width: 1440, height: 1000 });
    await page
      .getByRole("button", {
        name: `Play ${fixture.recording.title}`,
        exact: true,
      })
      .click();
    await expect
      .poll(() =>
        audio.evaluate((element: HTMLAudioElement) => element.currentTime),
      )
      .toBeGreaterThan(0.2);
    const created = await page.request.post("/api/v1/users", {
      headers,
      data: {
        username: "discovery-listener",
        password: "discovery listener password",
      },
    });
    expect(created.status()).toBe(201);
    const other = await browser.newContext({
      baseURL,
      storageState: { cookies: [], origins: [] },
    });
    try {
      const listener = await other.newPage();
      listener.on("pageerror", (error) => errors.push(error.message));
      await signIn(
        listener,
        "discovery-listener",
        "discovery listener password",
      );
      await listener.goto("/discover");
      await expect(
        listener.getByRole("link", {
          name: "Connect a music service",
          exact: true,
        }),
      ).toBeVisible();
      await expect(
        listener.getByRole("group", { name: "Music service" }),
      ).toHaveCount(0);
      expect(
        (
          await (
            await listener.request.get("/api/v1/recordings?q=Discovery")
          ).json()
        ).total,
      ).toBe(0);
      expect(
        (await (await listener.request.get("/api/v1/artists")).json()).total,
      ).toBe(0);
      expect(
        (await listener.request.get(`/api/v1/artists/${artistId}`)).status(),
      ).toBe(404);
    } finally {
      await other.close();
    }
    for (const request of fixture.requests) {
      expect(request.authorization).toBe(`Bearer ${fixture.token}`);
      expect(request.cookie).toBeUndefined();
      expect([fixture.source, fixture.secondSource]).toContain(request.source);
    }
    const removed = await page.request.delete(
      `/api/v1/plugins/${connection!.id}`,
      { headers, data: { revision: connection!.revision } },
    );
    expect(removed.ok()).toBe(true);
    connection = null;
    await page.goto("/library?q=Discovery");
    await expect(
      page.getByRole("button", {
        name: `Play ${fixture.recording.title}`,
        exact: true,
      }),
    ).toBeEnabled();
    await expect(
      page.getByRole("button", {
        name: `Play ${fixture.second.title}`,
        exact: true,
      }),
    ).toBeEnabled();
    await page.goto(`/artists/${artistId}`);
    await expect(
      page.getByRole("heading", {
        name: fixture.recording.artist,
        exact: true,
        level: 1,
      }),
    ).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Play tracks", exact: true }),
    ).toBeEnabled();
    await expect(
      page.getByRole("button", {
        name: `Play ${fixture.recording.title}`,
        exact: true,
      }),
    ).toBeEnabled();
    for (const title of [fixture.recording.title, fixture.second.title]) {
      const row = page.getByRole("row").filter({
        has: page.getByRole("button", { name: `Play ${title}`, exact: true }),
      });
      await expect(
        row.getByText("Not playable yet", { exact: true }),
      ).toBeVisible();
    }
    expect(errors).toEqual([]);
  } finally {
    if (connection)
      await page.request.delete(`/api/v1/plugins/${connection.id}`, {
        headers,
        data: { revision: connection.revision },
      });
    await fixture.close();
  }
});
