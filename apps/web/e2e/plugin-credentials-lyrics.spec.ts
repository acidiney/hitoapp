import { expect, test } from "@playwright/test";
import { ensureFixtureLibrary } from "./fixtures/credential-library";
import { enterAdmin, sharedAdminSession } from "./fixtures/admin-session";
import { libraryTrack } from "./fixtures/lyrics-library";
import {
  bumpRevision,
  connectCredentialPlugin,
  connectSavedLyrics,
  credentialLyricsPlugin,
  listLyricIdentities,
  pluginSettings,
} from "./fixtures/credential-plugin";
import {
  connectionCard,
  credentialMessage,
  ensureLyricsTab,
  expectAudioPlaying,
  expectQueueHolds,
  markLabel,
  networkQuiet,
  openLyricsFromLibrary,
  pluginsPath,
  playFromLibrary,
  settingsLinkName,
  showLyrics,
  trackTitle,
  openSettingsPlugins,
} from "./fixtures/credential-ui";

test.beforeAll(async ({ browser }, info) => {
  await ensureFixtureLibrary(browser, info.project.use.baseURL!);
});

const suffix = () => String(Date.now() % 1_000_000);

test("a lyric plugin that rejects its saved token shows credential guidance with the settings link and leaves playback, queue and the saved choice alone", async ({
  page,
  baseURL,
}) => {
  if (!sharedAdminSession) await enterAdmin(page);
  const origin = new URL(baseURL!).origin;
  const track = await libraryTrack(page, trackTitle);
  const setup = await connectSavedLyrics(
    page.request,
    page.request,
    origin,
    track,
    `Credential lyrics ${suffix()}`,
  );
  try {
    const saved = await listLyricIdentities(page.request, track.id);
    expect(saved).toHaveLength(1);
    setup.plugin.rotate();
    await playFromLibrary(page);
    const panel = await openLyricsFromLibrary(page);
    await showLyrics(panel);
    const card = panel.getByRole("alert");
    await expect(card).toContainText(credentialMessage);
    await expect(
      card.getByRole("link", { name: settingsLinkName }),
    ).toHaveAttribute("href", pluginsPath);
    await expect(card.getByRole("button", { name: "Retry" })).toBeVisible();
    const link = card.getByRole("link", { name: settingsLinkName });
    const another = card.getByRole("button", { name: "Try another plugin" });
    const look = (target: typeof link) =>
      target.evaluate((element) => {
        const style = getComputedStyle(element);
        const box = element.getBoundingClientRect();
        return {
          color: style.color,
          size: style.fontSize,
          weight: style.fontWeight,
          middle: Math.round(box.top + box.height / 2),
          height: Math.round(box.height),
        };
      });
    expect(await look(link)).toEqual(await look(another));
    await expect(page).toHaveURL(new RegExp(`/recordings/${track.id}`));
    await expect(
      page.getByRole("heading", { name: "Sign in", exact: true }),
    ).toHaveCount(0);
    await expectAudioPlaying(page);
    await expectQueueHolds(page);
    expect(await listLyricIdentities(page.request, track.id)).toEqual(saved);
    expect(setup.plugin.rejected("v2/lyrics/capabilities")).toBe(1);
    expect(setup.plugin.rejected("v2/lyrics")).toBe(0);
    expect(setup.plugin.accepted("v2/lyrics")).toBe(0);
  } finally {
    await setup.dispose();
  }
});

test("only the connection named by the stamp shows Credentials need updating in plugin settings and a healthy peer stays connected", async ({
  page,
  baseURL,
}) => {
  if (!sharedAdminSession) await enterAdmin(page);
  const origin = new URL(baseURL!).origin;
  const track = await libraryTrack(page, trackTitle);
  const label = `Credential lyrics ${suffix()}`;
  const peerLabel = `Credential peer ${suffix()}`;
  const setup = await connectSavedLyrics(
    page.request,
    page.request,
    origin,
    track,
    label,
  );
  const peer = await credentialLyricsPlugin(peerLabel);
  const peerConnection = await connectCredentialPlugin(
    page.request,
    page.request,
    origin,
    peer,
  );
  try {
    setup.plugin.rotate();
    await playFromLibrary(page);
    const panel = await openLyricsFromLibrary(page);
    await showLyrics(panel);
    await expect(panel.getByRole("alert")).toContainText(credentialMessage);
    await panel.getByRole("link", { name: settingsLinkName }).click();
    await expect(
      page.getByRole("heading", { name: "Plugins", exact: true }),
    ).toBeVisible();
    const affected = connectionCard(page, label);
    await expect(affected.getByText(markLabel)).toBeVisible();
    await expect(
      affected.getByRole("button", {
        name: `Update credentials for ${label}`,
      }),
    ).toBeVisible();
    const healthy = connectionCard(page, peerLabel);
    await expect(healthy).toBeVisible();
    await expect(healthy.getByText(markLabel)).toHaveCount(0);
    await expect(healthy.locator(".plugin-status")).toHaveText(/^Connected/);
    await expect(
      healthy.getByRole("button", { name: /^Update credentials/ }),
    ).toHaveCount(0);
    expect(peer.log.some((entry) => entry.outcome === "rejected")).toBe(false);
  } finally {
    await peerConnection.dispose();
    await setup.dispose();
  }
});

test("a fresh lyrics read that succeeds again with the same token clears the mark without a credential update", async ({
  page,
  baseURL,
}) => {
  if (!sharedAdminSession) await enterAdmin(page);
  const origin = new URL(baseURL!).origin;
  const track = await libraryTrack(page, trackTitle);
  const label = `Credential recovery ${suffix()}`;
  const setup = await connectSavedLyrics(
    page.request,
    page.request,
    origin,
    track,
    label,
  );
  try {
    setup.plugin.rotate();
    await playFromLibrary(page);
    await page.evaluate(() => {
      (
        window as unknown as { credentialRecovery?: string }
      ).credentialRecovery = "same-document";
    });
    const sameDocument = () =>
      page.evaluate(
        () =>
          (window as unknown as { credentialRecovery?: string })
            .credentialRecovery,
      );
    const panel = await openLyricsFromLibrary(page);
    await showLyrics(panel);
    await expect(panel.getByRole("alert")).toContainText(credentialMessage);
    await panel.getByRole("link", { name: settingsLinkName }).click();
    await expect(
      page.getByRole("heading", { name: "Plugins", exact: true }),
    ).toBeVisible();
    await expect(
      connectionCard(page, label).getByText(markLabel),
    ).toBeVisible();
    const revision = (await pluginSettings(page.request)).installations.find(
      (installation) => installation.manifest.name === label,
    )!.revision;
    const acceptedBefore = setup.plugin.accepted("v2/lyrics");

    setup.plugin.restore();
    const fresh = page.waitForResponse(
      (response) =>
        response.request().method() === "POST" &&
        response
          .url()
          .endsWith(`/recordings/${track.id}/lyrics/identities/read`) &&
        response.status() === 200,
    );
    await page.goBack();
    const reopened = await ensureLyricsTab(page);
    await showLyrics(reopened);
    const retry = reopened.getByRole("button", { name: "Retry" });
    if (await retry.isVisible()) await retry.click();
    const read = await (await fresh).json();
    expect(["ready_v1", "ready_v2"]).toContain(read.status);
    await expect(reopened.getByText("Plain river line")).toBeVisible();
    await expect(reopened.getByRole("alert")).toHaveCount(0);
    expect(setup.plugin.accepted("v2/lyrics")).toBeGreaterThan(acceptedBefore);

    await openSettingsPlugins(page);
    expect(await sameDocument()).toBe("same-document");
    expect(
      (await pluginSettings(page.request)).installations.find(
        (installation) => installation.manifest.name === label,
      )!.revision,
    ).toBe(revision);
    const card = connectionCard(page, label);
    await expect(card).toBeVisible();
    await expect(card.getByText(markLabel)).toHaveCount(0);
  } finally {
    await setup.dispose();
  }
});

test("updating credentials with the valid token clears the mark and the lyrics read succeeds with exactly one retry", async ({
  page,
  baseURL,
}) => {
  if (!sharedAdminSession) await enterAdmin(page);
  const origin = new URL(baseURL!).origin;
  const track = await libraryTrack(page, trackTitle);
  const label = `Credential lyrics ${suffix()}`;
  const setup = await connectSavedLyrics(
    page.request,
    page.request,
    origin,
    track,
    label,
  );
  try {
    setup.plugin.rotate();
    await playFromLibrary(page);
    const panel = await openLyricsFromLibrary(page);
    await showLyrics(panel);
    await expect(panel.getByRole("alert")).toContainText(credentialMessage);
    await panel.getByRole("link", { name: settingsLinkName }).click();
    const card = connectionCard(page, label);
    await expect(card.getByText(markLabel)).toBeVisible();
    await card
      .getByRole("button", { name: `Update credentials for ${label}` })
      .click();
    await card.getByLabel("New bearer token").fill(setup.plugin.currentToken());
    await card.getByRole("button", { name: "Save token" }).click();
    await expect(
      card.getByRole("status").filter({ hasText: "Connection checked." }),
    ).toBeVisible();
    await expect(card.getByText(markLabel)).toHaveCount(0);
    await expect(card.getByText(/needs its credentials updated/)).toHaveCount(
      0,
    );
    expect(setup.plugin.rejected("v2/lyrics/capabilities")).toBe(1);
    expect(setup.plugin.rejected("v2/lyrics")).toBe(0);

    await page.goBack();
    const back = await ensureLyricsTab(page);
    await showLyrics(back);
    await expect(back.getByText("Plain river line")).toBeVisible();
    await expect(back.getByText(credentialMessage)).toHaveCount(0);
    await networkQuiet(page);
    const settled = setup.plugin.log.length;
    await page.waitForTimeout(1_500);
    expect(setup.plugin.log).toHaveLength(settled);
    expect(setup.plugin.accepted("v2/lyrics")).toBe(1);
    expect(setup.plugin.rejected("v2/lyrics/capabilities")).toBe(1);
    expect(setup.plugin.rejected("v2/lyrics")).toBe(0);
    await expectAudioPlaying(page);
  } finally {
    await setup.dispose();
  }
});

test("after a stamped error a keep refresh raises the revision by one and the old mark no longer applies (the browser cannot receive a stale stamp from the backend, so that part stays in unit tests)", async ({
  page,
  baseURL,
}) => {
  if (!sharedAdminSession) await enterAdmin(page);
  const origin = new URL(baseURL!).origin;
  const track = await libraryTrack(page, trackTitle);
  const label = `Credential lyrics ${suffix()}`;
  const setup = await connectSavedLyrics(
    page.request,
    page.request,
    origin,
    track,
    label,
    "absent",
  );
  try {
    setup.plugin.rotate();
    await playFromLibrary(page);
    const panel = await openLyricsFromLibrary(page);
    await showLyrics(panel);
    const card = panel.getByRole("alert");
    await expect(card).toContainText(credentialMessage);
    await expect(
      card.getByRole("link", { name: settingsLinkName }),
    ).toHaveAttribute("href", pluginsPath);

    setup.plugin.state.manifestOpen = true;
    const bumped = await bumpRevision(page.request, origin, setup.installation);
    expect(bumped.revision).toBe(setup.installation.revision + 1);

    await card.getByRole("link", { name: settingsLinkName }).click();
    const connection = connectionCard(page, label);
    await expect(connection).toBeVisible();
    await expect(connection.getByText(markLabel)).toHaveCount(0);
    await expect(
      connection.getByText(/needs its credentials updated/),
    ).toHaveCount(0);
    await expect(connection.locator(".plugin-status")).toHaveText(/^Connected/);
    await expect(
      connection.getByRole("button", { name: /^Update credentials/ }),
    ).toHaveCount(0);
    const current = (await pluginSettings(page.request)).installations.find(
      (item) => item.id === setup.installation.id,
    );
    expect(current?.revision).toBe(bumped.revision);
    await expectAudioPlaying(page);
  } finally {
    await setup.dispose();
  }
});

test("credentials_required health renders on the affected connection and the token never reaches the console, network URLs or page text", async ({
  page,
  baseURL,
}) => {
  if (!sharedAdminSession) await enterAdmin(page);
  const origin = new URL(baseURL!).origin;
  const track = await libraryTrack(page, trackTitle);
  const label = `Credential lyrics ${suffix()}`;
  const setup = await connectSavedLyrics(
    page.request,
    page.request,
    origin,
    track,
    label,
  );
  const consoleText: string[] = [];
  const urls: string[] = [];
  page.on("console", (message) => consoleText.push(message.text()));
  page.on("request", (request) => urls.push(request.url()));
  const secrets = [setup.plugin.initialToken];
  try {
    secrets.push(setup.plugin.rotate());
    await expect
      .poll(
        async () =>
          (await pluginSettings(page.request)).installations.find(
            (item) => item.id === setup.installation.id,
          )?.health?.state,
      )
      .toBe("credentials_required");
    await page.goto(pluginsPath);
    const card = connectionCard(page, label);
    await expect(card.getByText(markLabel)).toBeVisible();
    await expect(
      card.getByText(`${label} needs its credentials updated.`).first(),
    ).toBeVisible();

    const leaks = async () => {
      const text = await page.locator("body").innerText();
      const html = await page.content();
      for (const secret of secrets) {
        expect(text).not.toContain(secret);
        expect(html).not.toContain(secret);
        expect(consoleText.join("\n")).not.toContain(secret);
        expect(urls.join("\n")).not.toContain(secret);
      }
    };
    await leaks();
    await card
      .getByRole("button", { name: `Update credentials for ${label}` })
      .click();
    const field = card.getByLabel("New bearer token");
    await expect(field).toHaveAttribute("type", "password");
    await field.fill(setup.plugin.currentToken());
    await leaks();
    await card.getByRole("button", { name: "Save token" }).click();
    await expect(
      card.getByRole("status").filter({ hasText: "Connection checked." }),
    ).toBeVisible();
    await expect(card.getByText(markLabel)).toHaveCount(0);
    await leaks();
  } finally {
    await setup.dispose();
  }
});

test("the Now Playing Lyrics panel shows the credential guidance for a rejected token while the audio keeps advancing", async ({
  page,
  baseURL,
}) => {
  if (!sharedAdminSession) await enterAdmin(page);
  await page.setViewportSize({ width: 1440, height: 900 });
  const origin = new URL(baseURL!).origin;
  const track = await libraryTrack(page, trackTitle);
  const setup = await connectSavedLyrics(
    page.request,
    page.request,
    origin,
    track,
    `Credential lyrics ${suffix()}`,
  );
  try {
    const saved = await listLyricIdentities(page.request, track.id);
    setup.plugin.rotate();
    await playFromLibrary(page);
    await page
      .getByRole("button", { name: "Open now playing", exact: true })
      .click();
    const dialog = page.getByRole("dialog", { name: "Now playing" });
    const lyrics = dialog.getByRole("region", { name: "Lyrics", exact: true });
    await expect(lyrics.getByText(credentialMessage)).toBeVisible();
    await expect(
      lyrics.getByRole("link", { name: settingsLinkName }),
    ).toHaveAttribute("href", pluginsPath);
    await expect(
      page.getByRole("heading", { name: "Sign in", exact: true }),
    ).toHaveCount(0);
    await expectAudioPlaying(page);
    expect(await listLyricIdentities(page.request, track.id)).toEqual(saved);
    expect(setup.plugin.rejected("v2/lyrics/capabilities")).toBeGreaterThan(0);
    expect(setup.plugin.rejected("v2/lyrics")).toBe(0);
    expect(setup.plugin.accepted("v2/lyrics")).toBe(0);
  } finally {
    await setup.dispose();
  }
});

test("a new token typed before a capability review is the one saved when the review is accepted, and Manage stays open meanwhile", async ({
  page,
  baseURL,
}) => {
  if (!sharedAdminSession) await enterAdmin(page);
  const origin = new URL(baseURL!).origin;
  const track = await libraryTrack(page, trackTitle);
  const label = `Credential review ${suffix()}`;
  const setup = await connectSavedLyrics(
    page.request,
    page.request,
    origin,
    track,
    label,
  );
  try {
    const token = setup.plugin.rotate();
    setup.plugin.state.addedCapabilities.push("artwork");
    await page.goto("/");
    await openSettingsPlugins(page);
    const card = connectionCard(page, label);
    const manage = card.getByRole("button", { name: "Manage", exact: true });
    await manage.click();
    await card.getByLabel("New bearer token").fill(token);
    await card.getByRole("button", { name: "Save token" }).click();
    const review = page.getByRole("dialog");
    await expect(review).toBeVisible();
    await expect(manage).toBeDisabled();
    await expect(card.getByLabel("New bearer token")).toBeVisible();
    const accepted = setup.plugin.accepted("v1/manifest");
    await review.getByRole("button", { name: /^Accept / }).click();
    await expect(
      card.getByRole("status").filter({ hasText: "Connection checked." }),
    ).toBeVisible();
    await expect(manage).toBeEnabled();
    expect(setup.plugin.accepted("v1/manifest")).toBeGreaterThan(accepted);
    await expect(page.getByText(token)).toHaveCount(0);
  } finally {
    await setup.dispose();
  }
});
