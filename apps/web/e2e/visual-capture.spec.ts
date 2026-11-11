import { expect, test, type Page } from "@playwright/test";
import { createPlaylist } from "./fixtures/playlist-dialogs";
import { openLibrarySection } from "./fixtures/library-sections";

const capture = process.env.HITO_VISUAL_CAPTURE === "1";
const outputFolder = "/workspace/test-results/visual";
const sizes = [
  { name: "1440", width: 1440, height: 900 },
  { name: "390", width: 390, height: 844 },
] as const;
const settingsSections = [
  "profile",
  "privacy",
  "music",
  "plugins",
  "playback",
  "downloads",
  "accounts",
  "updates",
  "about",
];

test.skip(!capture, "Set HITO_VISUAL_CAPTURE=1 to capture the screenshots");

async function shoot(page: Page, name: string, size: string) {
  await page.waitForTimeout(500);
  await page.screenshot({ path: `${outputFolder}/${name}-${size}.png` });
  const viewport = page.viewportSize()!;
  const height = await page.evaluate(
    () => document.documentElement.scrollHeight,
  );
  if (height > viewport.height) {
    await page.setViewportSize({ width: viewport.width, height });
    await page.waitForTimeout(300);
    await page.screenshot({ path: `${outputFolder}/${name}-${size}-full.png` });
    await page.setViewportSize(viewport);
  }
}

async function seedLibrary(page: Page) {
  await page.goto("/settings?section=music");
  for (const folder of ["Music", "Archive"]) {
    await page.getByRole("button", { name: "Add music folder" }).click();
    const picker = page.getByRole("dialog", { name: "Add a music folder" });
    await picker
      .getByRole("button", { name: `Open ${folder}`, exact: true })
      .click();
    await picker.getByRole("button", { name: "Use this folder" }).click();
    await expect(picker).not.toBeVisible();
  }
  await expect(
    page
      .getByRole("list", { name: "Connected music folders" })
      .getByRole("listitem")
      .filter({ hasText: /\d+ files? read/ }),
  ).toHaveCount(2, { timeout: 30000 });
  await page.goto("/library");
  const play = page.getByRole("button", { name: /^Play / }).first();
  await expect(play).toBeEnabled({ timeout: 20000 });
  await play.click();
  await expect
    .poll(() =>
      page
        .locator("audio[data-hito-player]")
        .evaluate((audio: HTMLAudioElement) => audio.currentTime),
    )
    .toBeGreaterThan(3);
  await page.getByRole("button", { name: "Pause", exact: true }).click();
  await page.waitForTimeout(1500);
  await page.getByRole("button", { name: "Play", exact: true }).click();
  await openLibrarySection(page, "Playlists");
  await createPlaylist(page, "Late night");
  await expect(
    page.locator(".playlist-list").getByText("Late night"),
  ).toBeVisible();
}

async function fillPlaylist(page: Page) {
  await page.goto("/library");
  const menus = page.getByRole("button", { name: /^Actions for / });
  await expect(menus.first()).toBeVisible();
  for (let index = 0; index < 4; index += 1) {
    await menus.nth(index).click();
    await page.getByRole("menuitem", { name: "Add to playlist" }).click();
    await page
      .getByRole("menu", { name: "Add to playlist" })
      .getByRole("menuitem", { name: "Late night", exact: true })
      .click();
    await page.waitForTimeout(400);
  }
  for (let index = 0; index < 3; index += 1) {
    await page
      .getByRole("button", { name: /^Like / })
      .first()
      .click();
    await page.waitForTimeout(400);
  }
}

async function captureCollections(page: Page) {
  await page.setViewportSize({ width: 1440, height: 900 });
  await fillPlaylist(page);
  for (const size of sizes) {
    await page.setViewportSize({ width: size.width, height: size.height });
    await page.goto("/library");
    await shoot(page, "library-tracks", size.name);
    await page.getByRole("button", { name: "List view" }).click();
    await shoot(page, "library-tracks-list", size.name);
    await page.goto("/albums");
    await shoot(page, "albums", size.name);
    await page.goto("/artists");
    await shoot(page, "artists", size.name);
    await page.goto("/playlists");
    await shoot(page, "playlists", size.name);
    await page.goto("/likes");
    await shoot(page, "likes", size.name);
    await page.goto("/history");
    await shoot(page, "history", size.name);
    await page.goto("/artists");
    await page
      .locator(".artist-grid a")
      .filter({ hasText: "Lume Areia" })
      .first()
      .click();
    await expect(page).toHaveURL(/\/artists\//);
    await shoot(page, "artist-credit", size.name);
    await page.goto("/albums");
    await page.locator(".album-grid a").first().click();
    await expect(page).toHaveURL(/\/albums\//);
    await shoot(page, "album", size.name);
    await page.goto("/playlists");
    await page
      .getByRole("link", { name: /Late night/ })
      .first()
      .click();
    await expect(page).toHaveURL(/\/playlists\//);
    await shoot(page, "playlist", size.name);
    const edit = page.getByRole("button", { name: "Edit", exact: true });
    if (await edit.isVisible()) {
      await edit.click();
      await shoot(page, "playlist-edit", size.name);
    }
  }
}

async function captureMedia(page: Page) {
  for (const size of sizes) {
    await page.setViewportSize({ width: size.width, height: size.height });
    await page.goto("/podcasts");
    await expect(
      page.getByRole("heading", { name: "Podcasts", exact: true }),
    ).toBeVisible();
    await shoot(page, "podcasts-empty", size.name);
    await page.getByRole("button", { name: "Subscribe to a podcast" }).click();
    await page
      .getByRole("dialog")
      .getByLabel("Feed address")
      .fill("not-a-feed.example/feed.xml");
    await shoot(page, "podcasts-subscribe", size.name);
    await page.keyboard.press("Escape");
    await page.goto("/audiobooks");
    await shoot(page, "audiobooks-empty", size.name);
    await page
      .getByRole("button", { name: "Add audiobook from a playlist" })
      .click();
    await page
      .getByRole("dialog")
      .getByRole("combobox", { name: "Playlist" })
      .selectOption({ label: "Late night" });
    await shoot(page, "audiobooks-add", size.name);
    await page.keyboard.press("Escape");
  }
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/audiobooks");
  await page
    .getByRole("button", { name: "Add audiobook from a playlist" })
    .click();
  const dialog = page.getByRole("dialog", { name: "Add an audiobook" });
  await dialog
    .getByRole("combobox", { name: "Playlist" })
    .selectOption({ label: "Late night" });
  await dialog
    .getByRole("button", { name: "Add audiobook", exact: true })
    .click();
  await expect(page).toHaveURL(/\/audiobooks\/[0-9a-f-]+$/);
  await page
    .getByRole("button", { name: "Play chapter 2 from beginning" })
    .click();
  await expect
    .poll(() =>
      page
        .locator("audio[data-hito-player]")
        .evaluate((audio: HTMLAudioElement) => audio.currentTime),
    )
    .toBeGreaterThan(2);
  await page.getByRole("button", { name: "Pause", exact: true }).click();
  await page.waitForTimeout(1500);
  await page.reload();
  const bookmarks = page.getByRole("region", { name: "Bookmarks" });
  await bookmarks
    .getByRole("combobox", { name: /Chapter/ })
    .selectOption({ index: 2 });
  await bookmarks.getByLabel("Position in seconds").fill("1.5");
  await bookmarks.getByLabel("Bookmark label").fill("Start of the good part");
  await bookmarks.getByRole("button", { name: "Save bookmark" }).click();
  await expect(bookmarks.getByText("Start of the good part")).toBeVisible();
  for (const size of sizes) {
    await page.setViewportSize({ width: size.width, height: size.height });
    await page.reload();
    await shoot(page, "audiobook", size.name);
    await page.goto("/audiobooks");
    await shoot(page, "audiobooks", size.name);
    await page.goBack();
  }
  await captureMockedPodcasts(page);
}

type InertiaPage = {
  component: string;
  props: Record<string, unknown>;
  url: string;
  version: string | null;
};

async function captureMockedPodcasts(page: Page) {
  await page.goto("/home");
  const base: InertiaPage = await page.evaluate(() =>
    JSON.parse(
      document.querySelector("script[data-page]")?.textContent ??
        document.getElementById("app")?.dataset.page ??
        "{}",
    ),
  );
  const id = "515993a6-776f-42bc-9d28-d4df99ceae28";
  const subscription = {
    subscription_id: id,
    title: "O Sal da Terra",
    refresh_pending: false,
    refresh_failed: false,
    last_refreshed_at: 1_790_000_000,
    content_revision: 1,
  };
  const names = [
    "43 · O pão da avó",
    "42 · Sal e vento",
    "41 · Mel de urze",
    "40 · A horta urbana",
    "39 · Queijo da Serra",
  ];
  const items = names.map((title, index) => ({
    episode_id: `00000000-0000-4000-8000-00000000000${index}`,
    recording: {
      id: `10000000-0000-4000-8000-00000000000${index}`,
      library_id: "20000000-0000-4000-8000-000000000000",
      title,
      inherited_title: title,
      artist: "O Sal da Terra",
      inherited_artist: "O Sal da Terra",
      duration_ms: 52 * 60_000,
      liked: false,
      available: true,
    },
  }));
  const progress = [
    { recording_id: items[1].recording.id, position_ms: 33 * 60_000, speed: 1 },
    { recording_id: items[2].recording.id, position_ms: 52 * 60_000, speed: 1 },
    { recording_id: items[3].recording.id, position_ms: 52 * 60_000, speed: 1 },
  ];
  const archive = {
    items,
    total: items.length,
    limit: 50,
    next_cursor: null,
    content_revision: 1,
  };
  const inertia = (
    component: string,
    url: string,
    extra: Record<string, unknown>,
  ) => ({ ...base, component, url, props: { ...base.props, ...extra } });
  await page.route(/\/(podcasts|api\/v1\/podcasts)(\/.*)?(\?.*)?$/, (route) => {
    const request = route.request();
    const path = new URL(request.url()).pathname;
    if (path === "/api/v1/podcasts")
      return route.fulfill({ json: [subscription] });
    if (path.endsWith("/episodes")) return route.fulfill({ json: archive });
    if (path.startsWith("/api/") || !request.headers()["x-inertia"])
      return route.fallback();
    const body =
      path === "/podcasts"
        ? inertia("PodcastIndexPage", path, { subscriptions: [subscription] })
        : inertia("PodcastPage", path, {
            subscription,
            episodes: archive,
            progress,
          });
    return route.fulfill({
      json: body,
      headers: { "x-inertia": "true", vary: "X-Inertia" },
    });
  });
  await page.route("**/api/v1/recordings/progress", (route) =>
    route.fulfill({ json: progress }),
  );
  for (const size of sizes) {
    await page.setViewportSize({ width: size.width, height: size.height });
    await page.goto("/home");
    await page
      .getByRole("link", { name: "Library", exact: true })
      .first()
      .click();
    await page
      .getByRole("link", { name: "Podcasts", exact: true })
      .first()
      .click();
    await expect(page.locator(".media-card-title")).toBeVisible();
    await shoot(page, "podcasts", size.name);
    await page.locator(".media-card-title").click();
    await expect(page.getByRole("list", { name: "Episodes" })).toBeVisible();
    await shoot(page, "podcast", size.name);
    await page.getByRole("radio", { name: "All" }).click();
    await shoot(page, "podcast-all", size.name);
  }
}

async function captureListening(page: Page) {
  for (const size of sizes) {
    await page.setViewportSize({ width: size.width, height: size.height });
    await page.goto("/library");
    const expand = page.getByRole("button", {
      name: "Expand player",
      exact: true,
    });
    if (!(await expand.isEnabled())) {
      await page
        .getByRole("button", { name: /^Play / })
        .first()
        .click();
      await expect(expand).toBeEnabled({ timeout: 20000 });
    }
    await expand.click();
    const dialog = page.getByRole("dialog", { name: "Now playing" });
    await expect(dialog).toBeVisible();
    await shoot(page, "now-playing", size.name);
    await page.keyboard.press("Escape");
    await expect(dialog).not.toBeVisible();
    await page.getByRole("button", { name: "Open queue", exact: true }).click();
    await expect(page.locator("#queue-panel[data-open]")).toBeVisible();
    await expect(
      page.getByRole("heading", { name: "Queue", level: 2 }),
    ).toBeVisible();
    await shoot(page, "queue", size.name);
  }
}

async function captureSetup(page: Page) {
  await page.route("**/", async (route) => {
    const response = await route.fetch();
    const body = await response.text();
    await route.fulfill({
      response,
      body: body.replace(
        /needs_setup(&quot;|")\s*:\s*false/,
        "needs_setup$1:true",
      ),
    });
  });
  for (const size of sizes) {
    await page.setViewportSize({ width: size.width, height: size.height });
    await page.goto("/");
    await expect(
      page.getByRole("heading", { name: "Set up Hito", exact: true }),
    ).toBeVisible();
    await shoot(page, "setup", size.name);
    await page.getByLabel("Username").fill("administrator");
    await page.getByLabel("Password").fill("short");
    await page.getByRole("button", { name: "Create administrator" }).click();
    await shoot(page, "setup-validation", size.name);
  }
}

test("capture every redesigned surface at desktop and phone width", async ({
  page,
  browser,
}) => {
  test.setTimeout(420000);
  await page.setViewportSize({ width: 1440, height: 900 });
  await seedLibrary(page);
  const tabs = page.getByRole("navigation", { name: "Main navigation" });
  for (const size of sizes) {
    await page.setViewportSize({ width: size.width, height: size.height });
    await tabs.getByRole("link", { name: "Home", exact: true }).click();
    await expect(page).toHaveURL(/\/home$/);
    await shoot(page, "home", size.name);
    await page
      .getByRole("link", { name: "Discover music from your services" })
      .click();
    await expect(page).toHaveURL(/\/discover/);
    await shoot(page, "discover", size.name);
    await tabs.getByRole("link", { name: /^Search/ }).click();
    await expect(page).toHaveURL(/\/search$/);
    await shoot(page, "search", size.name);
    await page.locator("#shell-search").fill("Lume");
    await page.keyboard.press("Enter");
    await page.waitForTimeout(800);
    await shoot(page, "search-results", size.name);
    await tabs.getByRole("link", { name: "Library", exact: true }).click();
    await expect(page).toHaveURL(/\/library$/);
    await shoot(page, "library", size.name);
    await page
      .getByRole("link", { name: "Account and settings", exact: true })
      .click();
    await expect(page).toHaveURL(/\/settings/);
    await shoot(page, "settings-index", size.name);
    const nav = page.getByRole("navigation", { name: "Settings sections" });
    for (const section of settingsSections) {
      if (!(await nav.isVisible()))
        await page.getByRole("link", { name: "All settings" }).click();
      await nav.locator(`a[href$="section=${section}"]`).click();
      await expect(page).toHaveURL(new RegExp(`section=${section}$`));
      await shoot(page, `settings-${section}`, size.name);
    }
  }
  await captureCollections(page);
  await captureMedia(page);
  await captureListening(page);
  const guest = await browser.newContext({
    storageState: { cookies: [], origins: [] },
  });
  const login = await guest.newPage();
  for (const size of sizes) {
    await login.setViewportSize({ width: size.width, height: size.height });
    await login.goto("/");
    await expect(
      login.getByRole("heading", { name: "Sign in", exact: true }),
    ).toBeVisible();
    await shoot(login, "sign-in", size.name);
  }
  await captureSetup(login);
  await guest.close();
});
