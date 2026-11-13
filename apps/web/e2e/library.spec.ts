import { signOutFromShell } from "./fixtures/account";
import { expect, test } from "@playwright/test";
import { createPlaylist } from "./fixtures/playlist-dialogs";
import { enterAdmin } from "./fixtures/admin-session";
import { chooseOption } from "./fixtures/listbox";
import { openLibrarySection } from "./fixtures/library-sections";
import { enterEditMode } from "./fixtures/playlist-edit";
import { addToPlaylistFromRow, trackTitleLink } from "./fixtures/track-row";

test.use({ storageState: { cookies: [], origins: [] } });

test("sign in, scan, listen across navigation, edit a playlist, and switch accounts", async ({
  page,
  context,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await enterAdmin(page);
  await page
    .getByRole("link", { name: "Account and settings", exact: true })
    .click();
  await page
    .getByRole("navigation", { name: "Settings sections" })
    .getByRole("link", { name: "Music", exact: true })
    .click();
  for (const folder of ["Music", "Archive"]) {
    await page.getByRole("button", { name: "Add music folder" }).click();
    const picker = page.getByRole("dialog", { name: "Add a music folder" });
    await picker
      .getByRole("button", { name: `Open ${folder}`, exact: true })
      .click();
    await expect(
      picker.getByText("This folder has no subfolders."),
    ).toBeVisible();
    await picker.getByRole("button", { name: "Use this folder" }).click();
    await expect(picker).not.toBeVisible();
    await expect(
      page.getByRole("button", { name: "Add music folder" }),
    ).toBeFocused();
  }
  await expect(
    page
      .getByRole("list", { name: "Connected music folders" })
      .getByRole("listitem"),
  ).toHaveCount(2);
  await expect(
    page
      .getByRole("list", { name: "Connected music folders" })
      .getByRole("listitem")
      .filter({ hasText: "1 file read" }),
  ).toHaveCount(2, { timeout: 20000 });
  await page.screenshot({
    path: "/workspace/test-results/folders-desktop.png",
  });
  await page
    .getByRole("navigation", { name: "Settings sections" })
    .getByRole("link", { name: "Music", exact: true })
    .click();
  await expect(page).toHaveURL(/section=music$/);
  await page.setViewportSize({ width: 390, height: 844 });
  await page.getByRole("button", { name: "Add music folder" }).click();
  await expect(
    page.getByRole("dialog", { name: "Add a music folder" }),
  ).toBeVisible();
  await page.screenshot({
    path: "/workspace/test-results/folder-picker-mobile.png",
  });
  await page.keyboard.press("Escape");
  await expect(
    page.getByRole("dialog", { name: "Add a music folder" }),
  ).not.toBeVisible();
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth > innerWidth,
    ),
  ).toBe(false);
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page
    .getByRole("navigation", { name: "Main navigation" })
    .getByRole("link", { name: "Library", exact: true })
    .click();
  await expect(
    page.getByRole("button", { name: "Play Fixture 440 Hz", exact: true }),
  ).toBeEnabled({ timeout: 20000 });
  await expect(
    page.locator(".track-table tbody tr").filter({ hasText: "Fixture" }),
  ).toHaveCount(2);
  await chooseOption(page.getByRole("combobox", { name: "Sort" }), "Title");
  await expect(page).toHaveURL(/\/library\?sort=title$/);
  await page
    .getByRole("button", { name: "Play Fixture 440 Hz", exact: true })
    .click();
  await expect
    .poll(() =>
      page
        .locator("audio[data-hito-player]")
        .evaluate((audio: HTMLAudioElement) => audio.currentTime),
    )
    .toBeGreaterThan(0.2);
  const before = await page
    .locator("audio[data-hito-player]")
    .evaluate((audio: HTMLAudioElement) => audio.currentTime);
  const playingSource = await page
    .locator("audio[data-hito-player]")
    .evaluate((audio: HTMLAudioElement) => audio.src);
  const navigation = page.getByRole("navigation", { name: "Main navigation" });
  await openLibrarySection(page, "Albums");
  await expect(
    page.getByRole("heading", { name: "Albums", exact: true }),
  ).toBeVisible();
  await expect(page.locator(".album-card")).toHaveCount(2);
  await page.screenshot({ path: "/workspace/test-results/albums-desktop.png" });
  await page
    .getByRole("link", { name: "Fixture collection 440 Hz", exact: true })
    .click();
  await expect(
    page.getByRole("heading", {
      name: "Fixture collection 440 Hz",
      exact: true,
    }),
  ).toBeVisible();
  await expect(page.locator(".track-table tbody tr")).toHaveCount(1);
  await page.screenshot({ path: "/workspace/test-results/album-desktop.png" });
  await page
    .locator("#main")
    .getByRole("link", { name: "Hito test fixtures", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "Hito test fixtures", exact: true }),
  ).toBeVisible();
  await expect(page.locator(".album-card")).toHaveCount(2);
  await page.setViewportSize({ width: 390, height: 844 });
  await page.screenshot({
    path: "/workspace/test-results/artist-mobile.png",
    fullPage: true,
  });
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth > innerWidth,
    ),
  ).toBe(false);
  await page.setViewportSize({ width: 1440, height: 1000 });
  await openLibrarySection(page, "Artists");
  await expect(
    page.locator(".artist-card").filter({ hasText: "Hito test fixtures" }),
  ).toHaveCount(1);
  await expect
    .poll(() =>
      page
        .locator("audio[data-hito-player]")
        .evaluate((audio: HTMLAudioElement) => audio.currentTime),
    )
    .toBeGreaterThan(before);
  await navigation.getByRole("link", { name: "Library", exact: true }).click();
  await expect(page.getByText("Made by you")).toHaveCount(0);
  await expect(
    page.locator(".track-table tbody tr").filter({ hasText: "Fixture" }),
  ).toHaveCount(2);
  await page.getByRole("button", { name: "Grid view", exact: true }).click();
  await expect(
    page.locator(".track-grid .track-card").filter({ hasText: "Fixture" }),
  ).toHaveCount(2);
  await trackTitleLink(page.locator(".track-grid"), "Fixture 440 Hz").click();
  await page.getByRole("tab", { name: /^Metadata/ }).click();
  await expect(
    page.getByRole("heading", { name: "Details", exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Edit track title" }).click();
  await page.getByRole("button", { name: "Edit track artist" }).click();
  await page
    .getByLabel("Track title", { exact: true })
    .fill("My fixture title");
  await page
    .getByLabel("Track artist", { exact: true })
    .fill("My fixture performer");
  let releaseRefresh!: () => void;
  let refreshStarted!: () => void;
  const refreshGate = new Promise<void>((resolve) => {
    releaseRefresh = resolve;
  });
  const refreshReached = new Promise<void>((resolve) => {
    refreshStarted = resolve;
  });
  const detailsUrl = page.url();
  await page.route(
    (url) => url.href === detailsUrl,
    async (route) => {
      const response = await route.fetch();
      refreshStarted();
      await refreshGate;
      await route.fulfill({ response });
    },
    { times: 1 },
  );
  await page
    .getByRole("button", { name: "Save personal title", exact: true })
    .click();
  await refreshReached;
  await expect(page.getByLabel("Track artist", { exact: true })).toBeDisabled();
  releaseRefresh();
  await expect(
    page.getByRole("heading", {
      name: "My fixture title",
      exact: true,
      level: 1,
    }),
  ).toBeVisible();
  await expect(page.locator(".now-playing strong")).toHaveText(
    "My fixture title",
  );
  await expect(page.getByLabel("Track artist", { exact: true })).toHaveValue(
    "My fixture performer",
  );
  await page
    .getByLabel("Track artist", { exact: true })
    .fill("My fixture performer");
  await page
    .getByRole("button", { name: "Save personal artist", exact: true })
    .click();
  await expect(page.locator(".now-playing small")).toHaveText(
    "My fixture performer",
  );
  await page.getByRole("button", { name: "Edit track artist" }).click();
  await expect(
    page.getByRole("button", { name: "Use source artist", exact: true }),
  ).toBeEnabled();
  await page.getByRole("tab", { name: /^History/ }).click();
  await page.getByText("Inspected tags", { exact: true }).click();
  await expect(page.locator(".metadata-tag-list")).toBeVisible();
  await page.screenshot({
    path: "/workspace/test-results/metadata-desktop.png",
    fullPage: true,
  });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.screenshot({
    path: "/workspace/test-results/metadata-mobile.png",
    fullPage: true,
  });
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth > innerWidth,
    ),
  ).toBe(false);
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.getByRole("tab", { name: /^Metadata/ }).click();
  await page.getByRole("button", { name: "Edit track title" }).click();
  await page
    .getByRole("button", { name: "Use source title", exact: true })
    .click();
  await expect(
    page.getByRole("heading", {
      name: "Fixture 440 Hz",
      exact: true,
      level: 1,
    }),
  ).toBeVisible();
  await expect(page.locator(".now-playing strong")).toHaveText(
    "Fixture 440 Hz",
  );
  await expect
    .poll(() =>
      page
        .locator("audio[data-hito-player]")
        .evaluate((audio: HTMLAudioElement) => audio.currentTime),
    )
    .toBeGreaterThan(before);
  await page
    .getByRole("button", { name: "Use source artist", exact: true })
    .click();
  await expect(page.locator(".now-playing small")).toHaveText(
    "Hito test fixtures",
  );
  await openLibrarySection(page, "Albums");
  await expect(page).toHaveURL(/\/albums$/);
  await page
    .getByRole("link", { name: "Fixture collection 440 Hz", exact: true })
    .click();
  await expect(page).toHaveURL(/\/albums\/[^/]+$/);
  const albumUrl = page.url();
  await page
    .locator(".album-header")
    .getByRole("button", { name: "More actions", exact: true })
    .click();
  await page
    .getByRole("menuitem", { name: "Edit album details", exact: true })
    .click();
  await page
    .getByLabel("Album title", { exact: true })
    .fill("My fixture album");
  await page
    .getByRole("button", { name: "Save personal title", exact: true })
    .click();
  await expect(
    page.getByRole("heading", {
      name: "My fixture album",
      exact: true,
      level: 1,
    }),
  ).toBeVisible();
  await page
    .getByLabel("Album artist", { exact: true })
    .fill("My album artist");
  await page
    .getByRole("button", { name: "Save personal artist", exact: true })
    .click();
  await expect(page.getByLabel("Album artist", { exact: true })).toHaveValue(
    "My album artist",
  );
  await expect(
    page.getByRole("button", { name: "Use source artist", exact: true }),
  ).toBeEnabled();
  await page.evaluate(() => window.scrollTo(0, 0));
  await page.screenshot({
    path: "/workspace/test-results/album-metadata-desktop.png",
    fullPage: true,
  });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.screenshot({
    path: "/workspace/test-results/album-metadata-mobile.png",
    fullPage: true,
  });
  await page
    .getByLabel("Album artist", { exact: true })
    .evaluate((input) =>
      input.closest("form")?.scrollIntoView({ block: "center" }),
    );
  await page.screenshot({
    path: "/workspace/test-results/album-metadata-controls-mobile.png",
  });
  const resetArtist = await page
    .getByRole("button", { name: "Use source artist", exact: true })
    .boundingBox();
  const playerDock = await page
    .getByRole("contentinfo", { name: "Audio player", exact: true })
    .boundingBox();
  expect(resetArtist).not.toBeNull();
  expect(playerDock).not.toBeNull();
  if (resetArtist && playerDock)
    expect(resetArtist.y + resetArtist.height).toBeLessThan(playerDock.y);
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth > innerWidth,
    ),
  ).toBe(false);
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.getByRole("link", { name: "Back to album", exact: true }).click();
  await expect(page).toHaveURL(albumUrl);
  await expect(
    page.getByRole("heading", {
      name: "My fixture album",
      exact: true,
      level: 1,
    }),
  ).toBeVisible();
  await expect(page.locator(".track-table")).toContainText(
    "Hito test fixtures",
  );
  await page
    .getByRole("link", { name: "My album artist", exact: true })
    .click();
  await expect(page.locator(".album-card")).toHaveCount(1);
  await page
    .getByRole("link", { name: "My fixture album", exact: true })
    .click();
  await page
    .locator(".album-header")
    .getByRole("button", { name: "More actions", exact: true })
    .click();
  await page
    .getByRole("menuitem", { name: "Edit album details", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Use source title", exact: true })
    .click();
  await expect(page.getByLabel("Album title", { exact: true })).toHaveValue(
    "Fixture collection 440 Hz",
  );
  await page
    .getByRole("button", { name: "Use source artist", exact: true })
    .click();
  await expect(page.getByLabel("Album artist", { exact: true })).toHaveValue(
    "Hito test fixtures",
  );
  await expect(page.locator(".now-playing small")).toHaveText(
    "Hito test fixtures",
  );
  expect(
    await page
      .locator("audio[data-hito-player]")
      .evaluate((audio: HTMLAudioElement) => audio.src),
  ).toBe(playingSource);
  expect(
    await page
      .locator("audio[data-hito-player]")
      .evaluate((audio: HTMLAudioElement) => audio.paused),
  ).toBe(false);
  await navigation.getByRole("link", { name: "Library", exact: true }).click();
  await page.screenshot({
    path: "/workspace/test-results/library-desktop.png",
  });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.screenshot({
    path: "/workspace/test-results/library-populated-mobile.png",
    fullPage: true,
  });
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth > innerWidth,
    ),
  ).toBe(false);
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page
    .getByRole("button", { name: "Expand player", exact: true })
    .click();
  const expanded = page.getByRole("dialog", {
    name: "Now playing",
    exact: true,
  });
  await expect(expanded).toBeVisible();
  await expect(
    expanded.getByRole("heading", { name: "Fixture 440 Hz", exact: true }),
  ).toBeVisible();
  await expect(expanded.locator(".np-next-row")).toHaveCount(1);
  await page.screenshot({
    path: "/workspace/test-results/now-playing-desktop.png",
  });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.screenshot({
    path: "/workspace/test-results/now-playing-mobile.png",
  });
  await page.setViewportSize({ width: 1440, height: 1000 });
  await expanded.getByRole("button", { name: "Close now playing" }).click();
  await expect(
    page.getByRole("button", { name: "Expand player", exact: true }),
  ).toBeFocused();
  await page
    .getByRole("button", { name: "Expand player", exact: true })
    .click();
  await page.keyboard.press("Escape");
  await expect(expanded).not.toBeVisible();
  await expect(
    page.getByRole("button", { name: "Expand player", exact: true }),
  ).toBeFocused();
  await page
    .getByRole("button", { name: "Expand player", exact: true })
    .click();
  await page.route("**/api/v1/recordings/*/playback-sources", (route) =>
    route.fulfill({ status: 503 }),
  );
  await expanded.locator(".np-next-row").last().click();
  await expect(expanded.getByRole("status")).toContainText(
    "Audio is unavailable",
  );
  await page.unroute("**/api/v1/recordings/*/playback-sources");
  await expanded.getByRole("link", { name: "Open queue" }).click();
  await expect(page.locator("#queue-panel[data-open]")).toBeVisible();
  await expect(
    page.getByRole("heading", { name: "Queue", exact: true, level: 2 }),
  ).toBeVisible();
  await page.getByText(/^Played \(/).click();
  await page
    .locator(".queue-track")
    .filter({ hasText: "Fixture" })
    .first()
    .click();
  await expect(page.locator("footer.player").getByRole("status")).toHaveCount(
    0,
  );
  await page
    .getByRole("button", {
      name: "Actions for Fixture 660 Hz: move up, move down, remove",
    })
    .click();
  await page.getByRole("menuitem", { name: "Remove", exact: true }).click();
  await page
    .getByRole("button", { name: "Remove Fixture 440 Hz from queue" })
    .click();
  const idle = page.getByText("Choose a track to start listening");
  if (!(await idle.isVisible())) {
    await page.getByRole("button", { name: "Clear", exact: true }).click();
    await page
      .getByRole("dialog")
      .getByRole("button", { name: "Clear queue" })
      .click();
  }
  await expect(idle).toBeVisible();
  await page
    .getByRole("navigation", { name: "Main navigation" })
    .getByRole("link", { name: "Library", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Play Fixture 440 Hz", exact: true })
    .click();
  await openLibrarySection(page, "Playlists");
  await expect(
    page.getByRole("heading", { name: "Playlists", exact: true }),
  ).toBeVisible();
  await expect
    .poll(() =>
      page
        .locator("audio[data-hito-player]")
        .evaluate((audio: HTMLAudioElement) => audio.currentTime),
    )
    .toBeGreaterThan(before);
  await createPlaylist(page, "Docker favorites");
  await expect(
    page.locator(".playlist-list").getByText("Docker favorites"),
  ).toBeVisible();
  await page
    .getByRole("navigation", { name: "Main navigation" })
    .getByRole("link", { name: "Library", exact: true })
    .click();
  const main = page.locator("#main");
  await main
    .getByRole("button", { name: "Like Fixture 440 Hz", exact: true })
    .click();
  await expect(
    main.getByRole("button", { name: "Unlike Fixture 440 Hz", exact: true }),
  ).toBeVisible();
  await addToPlaylistFromRow(
    page.locator(".track-table"),
    "Fixture 440 Hz",
    "Docker favorites",
  );
  await page
    .locator(".sidebar-playlists")
    .getByRole("link", { name: "Docker favorites" })
    .click();
  await expect(
    page.getByRole("heading", { name: "Docker favorites", exact: true }),
  ).toBeVisible();
  await enterEditMode(page);
  await expect(
    page.getByRole("button", {
      name: "Remove Fixture 440 Hz from playlist",
      exact: true,
    }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Pause", exact: true }).click();
  await openLibrarySection(page, "History");
  await expect(
    page.getByRole("button", { name: "Play Fixture 440 Hz", exact: true }),
  ).toBeVisible();
  await page
    .getByRole("link", { name: "Account and settings", exact: true })
    .click();
  const settingsNav = page.getByRole("navigation", {
    name: "Settings sections",
  });
  const profileLink = settingsNav.getByRole("link", {
    name: "Profile",
    exact: true,
  });
  await expect(profileLink).toHaveAttribute("aria-current", "page");
  const privacyLink = settingsNav.getByRole("link", {
    name: "Privacy & lock",
    exact: true,
  });
  await privacyLink.focus();
  await page.keyboard.press("Enter");
  await expect(privacyLink).toHaveAttribute("aria-current", "page");
  await expect(
    page.getByRole("region", { name: "Privacy & lock", exact: true }),
  ).toContainText("Report successful plays");
  await expect(
    page.getByRole("heading", { name: "Privacy & lock", level: 2 }),
  ).toBeFocused();
  await settingsNav
    .getByRole("link", { name: "Accounts", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Create account", exact: true })
    .click();
  const dialog = page.getByRole("dialog", { name: "Create account" });
  await dialog.getByLabel("Username", { exact: true }).fill("listener");
  await dialog
    .getByLabel("Password", { exact: true })
    .fill("listener docker test password");
  await dialog
    .getByRole("button", { name: "Create account", exact: true })
    .click();
  await expect(
    page
      .getByRole("region", { name: "People" })
      .getByText("listener", { exact: true }),
  ).toBeVisible();
  await page.evaluate(() => window.scrollTo(0, 0));
  await page.screenshot({
    path: "/workspace/test-results/settings-desktop.png",
    fullPage: true,
  });
  const connectionTab = await context.newPage();
  connectionTab.on("pageerror", (error) => errors.push(error.message));
  await connectionTab.route("**/api/v1/me", (route) =>
    route.fulfill({ status: 503 }),
  );
  await connectionTab.goto("/playlists");
  await expect(
    connectionTab.getByRole("heading", {
      name: "Cannot reach Hito",
    }),
  ).toBeVisible();
  await expect(connectionTab.locator(".playlist-list")).toHaveCount(0);
  await connectionTab.unroute("**/api/v1/me");
  await connectionTab
    .getByRole("button", { name: "Try now", exact: true })
    .click();
  await expect(
    connectionTab.locator(".playlist-list").getByText("Docker favorites"),
  ).toBeVisible();
  await connectionTab.close();
  const otherTab = await context.newPage();
  otherTab.on("pageerror", (error) => errors.push(error.message));
  await otherTab.goto("/library");
  await otherTab
    .getByRole("button", { name: "Play Fixture 440 Hz", exact: true })
    .click();
  await expect
    .poll(() =>
      otherTab
        .locator("audio[data-hito-player]")
        .evaluate((audio: HTMLAudioElement) => audio.currentTime),
    )
    .toBeGreaterThan(0);
  await openLibrarySection(otherTab, "Playlists");
  await expect(
    otherTab.locator(".playlist-list").getByText("Docker favorites"),
  ).toBeVisible();
  const delayedTab = await context.newPage();
  delayedTab.on("pageerror", (error) => errors.push(error.message));
  let releaseAsset!: () => void;
  let assetRequested!: () => void;
  const assetGate = new Promise<void>((resolve) => {
    releaseAsset = resolve;
  });
  const assetPaused = new Promise<void>((resolve) => {
    assetRequested = resolve;
  });
  await delayedTab.route("**/assets/app.js", async (route) => {
    assetRequested();
    await assetGate;
    await route.continue();
  });
  await delayedTab.goto("/playlists", { waitUntil: "commit" });
  await assetPaused;
  await expect
    .poll(() => delayedTab.locator("script[data-page=app]").textContent())
    .toContain("Docker favorites");
  await signOutFromShell(page);
  await expect(
    page.getByRole("heading", { name: "Sign in", exact: true }),
  ).toBeVisible();
  await expect(
    otherTab.getByRole("heading", { name: "Sign in", exact: true }),
  ).toBeVisible();
  await expect(otherTab.locator("audio")).toHaveCount(0);
  await page.getByLabel("Username", { exact: true }).fill("listener");
  await page
    .getByLabel("Password", { exact: true })
    .fill("listener docker test password");
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Your library", exact: true }),
  ).toBeVisible();
  await expect(page.getByText("0 tracks", { exact: true })).toBeVisible();
  const emptyLibrary = page.locator(".empty");
  await expect(
    emptyLibrary.getByRole("heading", { name: "Your library is waiting" }),
  ).toBeVisible();
  await expect(emptyLibrary).toContainText("An administrator adds");
  await expect(emptyLibrary.getByRole("link")).toHaveCount(0);
  await expect(page.getByRole("button", { name: /scan music/i })).toHaveCount(
    0,
  );
  releaseAsset();
  await expect(delayedTab.locator(".account")).toContainText("listener");
  await expect(
    delayedTab.getByText("Docker favorites", { exact: true }),
  ).toHaveCount(0);
  await delayedTab.close();
  await expect(otherTab.locator(".account")).toContainText("listener");
  await expect(
    otherTab.getByText("Docker favorites", { exact: true }),
  ).toHaveCount(0);
  await otherTab
    .getByRole("navigation", { name: "Main navigation" })
    .getByRole("link", { name: "Library", exact: true })
    .click();
  await otherTab.goBack();
  await expect(otherTab.locator(".account")).toContainText("listener");
  await expect(
    otherTab.getByText("Docker favorites", { exact: true }),
  ).toHaveCount(0);
  await otherTab.close();
  await expect(
    page.locator(".sidebar-playlists").getByText("Docker favorites"),
  ).toHaveCount(0);
  await page.goBack();
  await expect(page.getByText("Docker favorites", { exact: true })).toHaveCount(
    0,
  );
  await expect(page.locator(".account")).toContainText("listener");
  await page.setViewportSize({ width: 390, height: 844 });
  await page.screenshot({
    path: "/workspace/test-results/library-mobile.png",
    fullPage: true,
  });
  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth > innerWidth,
  );
  expect(overflow).toBe(false);
  expect(errors).toEqual([]);
});
