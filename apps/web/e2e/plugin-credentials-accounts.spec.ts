import { expect, test } from "@playwright/test";
import { ensureFixtureLibrary } from "./fixtures/credential-library";
import { enterAdmin, sharedAdminSession } from "./fixtures/admin-session";
import { signOutFromShell } from "./fixtures/account";
import { createMembers } from "./fixtures/member-accounts";
import { libraryTrack } from "./fixtures/lyrics-library";
import { trackTitleLink } from "./fixtures/track-row";
import {
  connectCredentialPlugin,
  connectSavedLyrics,
  credentialSourcesPlugin,
  installConnection,
  removeConnection,
} from "./fixtures/credential-plugin";
import {
  accessLostMessage,
  audioTime,
  chooseService,
  connectionCard,
  credentialMessage,
  ensureLyricsTab,
  lyricsPanel,
  memberSession,
  networkQuiet,
  openSettingsPlugins,
  settingsLinkName,
  showLyrics,
  submitSignIn,
  trackTitle,
  markLabel,
} from "./fixtures/credential-ui";

const suffix = () => String(Date.now() % 1_000_000);

test.beforeAll(async ({ browser }, info) => {
  await ensureFixtureLibrary(browser, info.project.use.baseURL!);
});

test.describe("account switching and listener sessions", () => {
  test.use({ storageState: { cookies: [], origins: [] } });

  test("a rejection held for account A lands on neither account B nor A when A returns", async ({
    page,
    browser,
    baseURL,
  }) => {
    const origin = new URL(baseURL!).origin;
    const members = await createMembers(browser, baseURL!, "cred", 2);
    const [nameA, nameB] = members.names;
    const contextA = await memberSession(browser, baseURL!, nameA);
    const contextB = await memberSession(browser, baseURL!, nameB);
    const label = `Credential lyrics ${suffix()}`;
    let setup: Awaited<ReturnType<typeof connectSavedLyrics>> | null = null;
    let installationB: Awaited<ReturnType<typeof installConnection>> | null =
      null;
    try {
      await page.goto("/");
      await submitSignIn(page, nameA);
      const track = await libraryTrack(page, trackTitle);
      setup = await connectSavedLyrics(
        members.administrator,
        contextA.request,
        origin,
        track,
        label,
        "absent",
      );
      const tokenB = setup.plugin.rotate();
      installationB = await installConnection(
        contextB.request,
        origin,
        setup.service,
        tokenB,
      );
      setup.plugin.hold("v2/lyrics");
      await page.goto(`/recordings/${track.id}?tab=lyrics`);
      await showLyrics(lyricsPanel(page));
      await setup.plugin.held("v2/lyrics");

      await signOutFromShell(page);
      await submitSignIn(page, nameB);
      setup.plugin.release("v2/lyrics");
      await expect.poll(() => setup!.plugin.rejected("v2/lyrics")).toBe(1);
      await networkQuiet(page);

      await trackTitleLink(page.locator("main"), trackTitle).first().click();
      const panelB = await ensureLyricsTab(page);
      await networkQuiet(page);
      await expect(panelB.getByText(credentialMessage)).toHaveCount(0);
      await expect(
        panelB.getByRole("link", { name: settingsLinkName }),
      ).toHaveCount(0);
      await openSettingsPlugins(page);
      const cardB = connectionCard(page, label);
      await expect(cardB).toBeVisible();
      await expect(cardB.getByText(markLabel)).toHaveCount(0);
      await expect(cardB.locator(".plugin-status")).toHaveText(/^Connected/);

      await signOutFromShell(page);
      await submitSignIn(page, nameA);
      await openSettingsPlugins(page);
      const cardA = connectionCard(page, label);
      await expect(cardA).toBeVisible();
      await expect(cardA.getByText(markLabel)).toHaveCount(0);
      await expect(cardA.locator(".plugin-status")).toHaveText(/^Connected/);
    } finally {
      if (installationB)
        await removeConnection(contextB.request, origin, installationB);
      await setup?.dispose();
      await contextA.close();
      await contextB.close();
      await members.close();
    }
  });

  test("an expired listener session still goes to sign-in guidance and shows no credential feedback", async ({
    page,
    browser,
    baseURL,
  }) => {
    const origin = new URL(baseURL!).origin;
    const members = await createMembers(browser, baseURL!, "cred401", 1);
    const [name] = members.names;
    const context = await memberSession(browser, baseURL!, name);
    let setup: Awaited<ReturnType<typeof connectSavedLyrics>> | null = null;
    try {
      await page.goto("/");
      await submitSignIn(page, name);
      const track = await libraryTrack(page, trackTitle);
      setup = await connectSavedLyrics(
        members.administrator,
        context.request,
        origin,
        track,
        `Credential lyrics ${suffix()}`,
      );
      await page.goto(`/recordings/${track.id}?tab=lyrics`);
      const panel = lyricsPanel(page);
      const show = panel.getByRole("button", { name: "Show lyrics" });
      await expect(show).toBeVisible();
      await page.context().clearCookies();
      await show.click();
      await expect(panel.getByRole("alert")).toContainText(accessLostMessage);
      await expect(panel.getByText(credentialMessage)).toHaveCount(0);
      await expect(
        panel.getByRole("link", { name: settingsLinkName }),
      ).toHaveCount(0);
      expect(setup.plugin.accepted("v2/lyrics")).toBe(0);
      await page.reload();
      await expect(
        page.getByRole("heading", { name: "Sign in", exact: true }),
      ).toBeVisible();
    } finally {
      await setup?.dispose();
      await context.close();
      await members.close();
    }
  });
});

test("an unstamped credential 409 from Discover search shows generic guidance with the settings link and marks no connection", async ({
  page,
  baseURL,
}) => {
  if (!sharedAdminSession) await enterAdmin(page);
  const origin = new URL(baseURL!).origin;
  const label = `Unstamped relay ${suffix()}`;
  const plugin = await credentialSourcesPlugin(label, "absent");
  const connection = await connectCredentialPlugin(
    page.request,
    page.request,
    origin,
    plugin,
  );
  try {
    plugin.state.manifestOpen = true;
    plugin.rotate();
    await page.goto("/discover");
    const search = page.getByRole("search", { name: "Music service search" });
    await search
      .getByRole("searchbox", { name: "Search music" })
      .fill("Relay");
    await search.getByRole("button", { name: "Search", exact: true }).click();
    await expect(page.getByText(credentialMessage).first()).toBeVisible();
    const link = page.getByRole("link", { name: settingsLinkName });
    await expect(link).toHaveAttribute("href", "/settings?section=plugins");
    await expect(
      page.getByRole("heading", { name: "Sign in", exact: true }),
    ).toHaveCount(0);
    expect(plugin.rejected("v1/search")).toBeGreaterThan(0);
    expect(plugin.accepted("v1/search")).toBe(0);
    await link.click();
    const card = connectionCard(page, label);
    await expect(card).toBeVisible();
    await expect(card.getByText(markLabel)).toHaveCount(0);
    await expect(card.getByText(/needs its credentials updated/)).toHaveCount(
      0,
    );
    await expect(card.locator(".plugin-status")).toHaveText(/^Connected/);
  } finally {
    await connection.dispose();
  }
});

test("provider search and source attachment show the credential guidance for the rejecting plugin while the healthy provider still resolves sources", async ({
  page,
  baseURL,
}) => {
  if (!sharedAdminSession) await enterAdmin(page);
  const origin = new URL(baseURL!).origin;
  const stamp = suffix();
  const rejectingLabel = `Rejecting relay ${stamp}`;
  const healthyLabel = `Healthy relay ${stamp}`;
  const rejecting = await credentialSourcesPlugin(rejectingLabel);
  const healthy = await credentialSourcesPlugin(healthyLabel);
  const rejectingConnection = await connectCredentialPlugin(
    page.request,
    page.request,
    origin,
    rejecting,
  );
  const healthyConnection = await connectCredentialPlugin(
    page.request,
    page.request,
    origin,
    healthy,
  );
  try {
    rejecting.rotate();
    await page.goto("/discover");
    const search = page.getByRole("search", { name: "Music service search" });
    const query = search.getByRole("searchbox", { name: "Search music" });
    await chooseService(search, rejectingLabel);
    await query.fill(`Relay ${stamp}`);
    await search.getByRole("button", { name: "Search", exact: true }).click();
    await expect(page.getByText(credentialMessage).first()).toBeVisible();
    await expect(
      page.getByRole("link", { name: settingsLinkName }),
    ).toHaveAttribute("href", "/settings?section=plugins");
    expect(rejecting.rejected("v1/search")).toBeGreaterThan(0);
    expect(rejecting.accepted("v1/search")).toBe(0);

    await chooseService(search, healthyLabel);
    await query.fill(`Relay ${stamp}`);
    await search.getByRole("button", { name: "Search", exact: true }).click();
    const title = healthy.inner.recording.title;
    await expect(
      page.getByRole("button", { name: `Play ${title}`, exact: true }),
    ).toBeEnabled();
    await expect(page.getByText(credentialMessage)).toHaveCount(0);
    await page
      .getByRole("button", { name: `Add ${title} to library`, exact: true })
      .click();
    await page
      .getByRole("button", { name: `Play ${title}`, exact: true })
      .click();
    await expect.poll(() => audioTime(page)).toBeGreaterThan(0.2);
    expect(healthy.inner.sourceRequests.length).toBeGreaterThan(0);
    expect(rejecting.accepted("v1/sources")).toBe(0);

    await page
      .getByRole("link", { name: `View ${title} in library`, exact: true })
      .click();
    await page.getByRole("tab", { name: "Sources" }).click();
    const attach = page.getByRole("region", {
      name: /^(Find audio|Add another source)$/,
    });
    await expect(attach).toBeVisible();
    const open = attach.getByRole("button", { name: "Find a source" });
    const form = attach.getByRole("search", { name: "Music service search" });
    await expect(open.or(form)).toBeVisible();
    if (await open.isVisible()) await open.click();
    await expect(form).toBeVisible();
    await chooseService(attach, rejectingLabel);
    await form.getByRole("searchbox", { name: "Search music" }).fill(stamp);
    await form.getByRole("button", { name: "Search", exact: true }).click();
    await expect(attach.getByRole("alert")).toContainText(credentialMessage);
    await expect(
      attach.getByRole("alert").getByRole("link", { name: settingsLinkName }),
    ).toBeVisible();

    await chooseService(attach, healthyLabel);
    await form.getByRole("searchbox", { name: "Search music" }).fill(stamp);
    await form.getByRole("button", { name: "Search", exact: true }).click();
    await expect(
      attach
        .getByRole("list", { name: "Source search results" })
        .getByRole("button", { name: "Review match" })
        .first(),
    ).toBeVisible();
    await expect(attach.getByText(credentialMessage)).toHaveCount(0);
    expect(rejecting.accepted("v1/search")).toBe(0);
    expect(rejecting.accepted("v1/sources")).toBe(0);
  } finally {
    await healthyConnection.dispose();
    await rejectingConnection.dispose();
  }
});
