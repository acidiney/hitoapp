import { openAllManage } from "./fixtures/plugin-ui";
import { isDeepStrictEqual } from "node:util";
import { expect, test, type BrowserContext } from "@playwright/test";
import type { Schema } from "../src/shared/schema";
import { reviewPluginInstallation } from "./fixtures/plugin-installation-review";
import { metadataPreferencesProvider } from "./fixtures/metadata-preferences-provider";
import { preferencesEvidence } from "./fixtures/metadata-preferences-evidence";
import {
  addMetadataProvider,
  openPreferences,
  removeMetadataProvider,
  preferencesAction,
  preferencesLibraryState,
  preferencesReader,
  savePreferences,
  seedPreferencesLibrary,
} from "./fixtures/metadata-preferences-state";

const builtin = {
  kind: "builtin_musicbrainz",
} satisfies Schema<"MetadataProvider">;
test.use({
  trace: "off",
  screenshot: "off",
  video: "off",
  launchOptions: { args: [] },
});
test.skip(
  process.env.HITO_METADATA_PREFERENCES_E2E !== "1",
  "Requires an opted-in fresh disposable metadata preferences server",
);

test("account metadata preferences persist ordered personal providers without touching music or fetching profiles", async ({
  page,
  context,
  browser,
  baseURL,
}) => {
  test.setTimeout(120_000);
  page.setDefaultTimeout(10_000);
  page.setDefaultNavigationTimeout(15_000);
  let stage = "fresh disposable server";
  let diagnostics: Record<string, boolean | number> = {};
  let failed = false;
  let provider: Awaited<ReturnType<typeof metadataPreferencesProvider>> | null =
    null;
  let otherContext: BrowserContext | null = null;
  const password = "owned preferences administrator password";
  const otherPassword = "owned preferences listener password";
  const privateValues = [
    password,
    otherPassword,
    "/music/",
    "Fixture 440 Hz.wav",
    "Fixture 660 Hz.wav",
    "Fixture 440 Hz",
    "Fixture 660 Hz",
    "Hito test fixtures",
    "Fixture collection 440 Hz",
    "Fixture collection 660 Hz",
  ];
  const evidence = await preferencesEvidence(
    baseURL ?? "",
    privateValues,
    () => stage,
  );
  const read = preferencesReader(page);
  try {
    expect(baseURL === "http://localhost:8080").toBe(true);
    expect(
      (await read<Schema<"SetupStatus">>("/api/v1/setup")).needs_setup,
    ).toBe(true);
    await evidence.observe(context);
    provider = await metadataPreferencesProvider();
    const owned = provider;
    privateValues.push(
      owned.name,
      owned.origin,
      ...owned.manifests.map((manifest) => manifest.id),
    );
    await page.goto("/");
    await page.getByLabel("Username", { exact: true }).fill("admin");
    await page.getByLabel("Password", { exact: true }).fill(password);
    await page
      .getByRole("button", { name: "Create administrator", exact: true })
      .click();
    await expect(
      page.getByRole("heading", { name: "Your library", exact: true }),
    ).toBeVisible();
    const user = await read<Schema<"User">>("/api/v1/me");
    privateValues.push(
      ...(await context.cookies()).map((cookie) => cookie.value),
    );

    stage = "owned originals and personal state";
    const saved = await seedPreferencesLibrary(page, read, user);
    privateValues.push(...saved.privateValues);
    const before = await preferencesLibraryState(page, read, saved);
    expect(
      before.choices.every(
        (choice) => choice.status === 404 && choice.choice === null,
      ),
    ).toBe(true);
    expect(
      before.playlist.entries.length === 2 &&
        before.playlist.entries.every(
          (entry) => entry.recording_id === saved.ids[0],
        ) &&
        isDeepStrictEqual(
          before.playlist.entries.map((entry) => entry.entry_id),
          saved.entries,
        ) &&
        before.metadata[0].title.personal === saved.privateValues[0] &&
        before.metadata[0].artist.personal === saved.privateValues[1] &&
        before.catalog.items.find((item) => item.id === saved.ids[0])?.liked ===
          true &&
        before.progress.some(
          (item) =>
            item.recording_id === saved.ids[0] &&
            item.position_ms === 400 &&
            item.speed === 1.25,
        ) &&
        before.sources.every((sources) => sources.length === 1),
    ).toBe(true);

    stage = "default provider and explicit disable";
    let panel = await openPreferences(page);
    const defaults = {
      revision: 0,
      enabled: true,
      automatic_artist_lookup: true,
      automatic_artwork: true,
      automatic_service_search: true,
      providers: [builtin],
    } satisfies Schema<"MetadataSettings">;
    expect(
      isDeepStrictEqual(await read("/api/v1/metadata/settings"), defaults),
    ).toBe(true);
    await expect(
      panel.getByRole("switch", {
        name: "Use metadata providers",
        exact: true,
      }),
    ).toBeChecked();
    await expect(
      panel.getByRole("list", {
        name: "Selected metadata providers",
        exact: true,
      }),
    ).toHaveText(/MusicBrainz/);
    await panel
      .getByRole("switch", { name: "Use metadata providers", exact: true })
      .uncheck();
    expect(
      isDeepStrictEqual(await read("/api/v1/metadata/settings"), defaults),
    ).toBe(true);
    await savePreferences(page, { ...defaults, enabled: false });
    panel = await openPreferences(page);
    await expect(
      panel.getByRole("switch", {
        name: "Use metadata providers",
        exact: true,
      }),
    ).not.toBeChecked();
    expect(owned.calls.total).toBe(0);

    stage = "address-only offers and personal catalog approval";
    await page.goto("/settings?section=plugins");
    await page.getByRole("button", { name: /^This server/ }).click();
    const connections: Schema<"PluginInstallation">[] = [];
    for (const [index, address] of owned.addresses.entries()) {
      await page.getByRole("button", { name: /^This server/ }).click();
      await page.getByLabel("Service address", { exact: true }).fill(address);
      const offered = await preferencesAction<Schema<"PluginService">>(
        page,
        page.getByRole("button", { name: "Offer service", exact: true }),
        "POST",
        "/api/v1/plugins/services",
        201,
      );
      expect(
        isDeepStrictEqual(offered.sent, { base_url: address }) &&
          offered.body?.name === owned.name,
      ).toBe(true);
      if (!offered.body)
        throw new Error("The offered catalog identity is missing");
      await page
        .getByRole("button", { name: "Your plugins", exact: true })
        .click();
      await page
        .getByRole("button", {
          name: `Connect ${offered.body.name}`,
          exact: true,
        })
        .click();
      const reviewed = await reviewPluginInstallation(
        page,
        offered.body,
        owned.name,
      );
      const installed = await preferencesAction<Schema<"PluginInstallation">>(
        page,
        page.getByRole("button", { name: "Accept and connect", exact: true }),
        "POST",
        "/api/v1/plugins",
        201,
      );
      if (!installed.body)
        throw new Error("The approved catalog identity is missing");
      expect(
        installed.body.service_id === offered.body.id &&
          installed.body.manifest.id === owned.manifests[index].id &&
          installed.body.manifest.capabilities.join(",") === "catalog" &&
          !installed.body.credential_set &&
          isDeepStrictEqual(installed.sent, {
            ...reviewed.sent,
            accept_manifest_digest: reviewed.body.digest,
          }),
      ).toBe(true);
      connections.push(installed.body);
      privateValues.push(installed.body.id, offered.body.id);
    }
    expect(
      connections[0].id !== connections[1].id &&
        connections[0].service_id !== connections[1].service_id,
    ).toBe(true);
    expect(
      isDeepStrictEqual(owned.calls.manifest, [3, 3]) &&
        owned.calls.total === 6,
    ).toBe(true);
    const callsAfterApproval = structuredClone(owned.calls);

    stage = "ordered same-name providers and replacement";
    const first = {
      kind: "http_plugin",
      installation_id: connections[0].id,
    } satisfies Schema<"MetadataProvider">;
    const second = {
      kind: "http_plugin",
      installation_id: connections[1].id,
    } satisfies Schema<"MetadataProvider">;
    panel = await openPreferences(page);
    const selected = panel.getByRole("list", {
      name: "Selected metadata providers",
      exact: true,
    });
    for (const connection of connections) {
      await addMetadataProvider(panel, 0);
    }
    await expect(selected.getByRole("listitem")).toHaveCount(3);
    await selected
      .getByRole("listitem")
      .nth(2)
      .getByRole("button", { name: `Move ${owned.name}`, exact: true })
      .click();
    await page.getByRole("menuitem", { name: "Move earlier" }).click();
    await selected
      .getByRole("listitem")
      .nth(1)
      .getByRole("button", { name: `Move ${owned.name}`, exact: true })
      .click();
    await page.getByRole("menuitem", { name: "Move earlier" }).click();
    await panel
      .getByRole("switch", { name: "Use metadata providers", exact: true })
      .check();
    await savePreferences(page, {
      revision: 1,
      enabled: true,
      providers: [second, builtin, first],
    });
    panel = await openPreferences(page);
    expect(
      isDeepStrictEqual(await read("/api/v1/metadata/settings"), {
        ...defaults,
        revision: 2,
        enabled: true,
        providers: [second, builtin, first],
      }),
    ).toBe(true);
    await removeMetadataProvider(panel, "MusicBrainz");
    await removeMetadataProvider(
      panel
        .getByRole("list", { name: "Selected metadata providers", exact: true })
        .getByRole("listitem")
        .nth(1),
      owned.name,
    );
    await savePreferences(page, {
      revision: 2,
      enabled: true,
      providers: [second],
    });
    panel = await openPreferences(page);
    await expect(
      panel
        .getByRole("list", { name: "Selected metadata providers", exact: true })
        .getByRole("listitem"),
    ).toHaveCount(1);

    stage = "second account defaults and private connection authority";
    await page
      .getByRole("navigation", { name: "Settings sections" })
      .getByRole("link", { name: "Accounts", exact: true })
      .click();
    await page
      .getByRole("button", { name: "Create account", exact: true })
      .click();
    const dialog = page.getByRole("dialog", { name: "Create account" });
    await dialog
      .getByLabel("Username", { exact: true })
      .fill("preferences-listener");
    await dialog.getByLabel("Password", { exact: true }).fill(otherPassword);
    const created = await preferencesAction<Schema<"User">>(
      page,
      dialog.getByRole("button", { name: "Create account", exact: true }),
      "POST",
      "/api/v1/users",
      201,
    );
    expect(created.body?.admin === false).toBe(true);
    stage = "create isolated second account context";
    otherContext = await browser.newContext({ baseURL });
    stage = "register second account privacy and logging init scripts";
    await evidence.observe(otherContext);
    stage = "create second account blank page with init scripts";
    const other = await otherContext.newPage();
    other.setDefaultTimeout(10_000);
    stage = "navigate second account to owned sign-in page";
    await other.goto("/");
    stage = "sign in isolated second account";
    await other
      .getByLabel("Username", { exact: true })
      .fill("preferences-listener");
    await other.getByLabel("Password", { exact: true }).fill(otherPassword);
    await other.getByRole("button", { name: "Sign in", exact: true }).click();
    await expect(
      other.getByRole("heading", { name: "Your library", exact: true }),
    ).toBeVisible();
    privateValues.push(
      ...(await otherContext.cookies()).map((cookie) => cookie.value),
    );
    stage = "read second account defaults and private catalog";
    const otherRead = preferencesReader(other);
    const otherBefore = {
      preferences: await otherRead<Schema<"MetadataSettings">>(
        "/api/v1/metadata/settings",
      ),
      catalog: await otherRead<Schema<"RecordingPage">>("/api/v1/recordings"),
      playlists: await otherRead<Schema<"Playlist">[]>("/api/v1/playlists"),
      activity: await otherRead<Schema<"Activity">>("/api/v1/activity"),
    };
    expect(
      isDeepStrictEqual(otherBefore.preferences, defaults) &&
        otherBefore.catalog.total === 0 &&
        otherBefore.playlists.length === 0 &&
        otherBefore.activity.history.length === 0,
    ).toBe(true);
    stage = "show second account metadata preference authority";
    const otherPanel = await openPreferences(other);
    await expect(
      other
        .getByRole("navigation", { name: "Settings sections" })
        .getByRole("link", { name: "Accounts", exact: true }),
    ).toHaveCount(0);
    await expect(
      otherPanel.getByRole("switch", {
        name: "Use metadata providers",
        exact: true,
      }),
    ).toBeChecked();
    await expect(
      otherPanel.getByRole("button", { name: "Add provider", exact: true }),
    ).toHaveCount(0);
    const otherPlugins =
      await otherRead<Schema<"PluginSettings">>("/api/v1/plugins");
    expect(
      otherPlugins.installations.length === 0 &&
        otherPlugins.services.length === 2,
    ).toBe(true);
    await other.goto("/settings?section=plugins");
    await expect(
      other.getByText("No plugins connected to this account.", { exact: true }),
    ).toBeVisible();
    await expect(
      other.getByText("Services for this server", { exact: true }),
    ).toHaveCount(0);

    stage = "disconnect selected provider and disable retained reference";
    await page.goto("/settings?section=plugins");
    const installedSettings =
      await read<Schema<"PluginSettings">>("/api/v1/plugins");
    const selectedIndex = installedSettings.installations.findIndex(
      (item) => item.id === connections[1].id,
    );
    expect(selectedIndex >= 0).toBe(true);
    const connection = page.locator(".plugin-connection").nth(selectedIndex);
    await expect(page.locator(".plugin-connection")).toHaveCount(2);
    await openAllManage(page);
    await connection
      .getByRole("button", { name: "Remove", exact: true })
      .click();
    stage = "selected provider removal awaiting accepted response";
    await openAllManage(page);
    const removed = await preferencesAction(
      page,
      page
        .getByRole("dialog")
        .getByRole("button", { name: "Remove", exact: true }),
      "DELETE",
      `/api/v1/plugins/${connections[1].id}`,
      204,
      (phase) => {
        stage =
          phase === "accepted_response"
            ? "selected provider removal accepted204 before body read"
            : phase === "no_content"
              ? "selected provider removal204 completed without body"
              : "selected provider removal body read before request decode";
      },
    );
    expect(
      removed.body === null &&
        isDeepStrictEqual(removed.sent, {
          revision: installedSettings.installations[selectedIndex].revision,
        }),
    ).toBe(true);
    await expect(page.locator(".plugin-connection")).toHaveCount(1);
    const afterRemoval =
      await read<Schema<"PluginSettings">>("/api/v1/plugins");
    expect(
      afterRemoval.installations.length === 1 &&
        afterRemoval.installations.every(
          (item) => item.id !== connections[1].id,
        ) &&
        afterRemoval.installations[0].id === connections[0].id,
    ).toBe(true);
    stage = "selected provider removed before unavailable settings reload";
    panel = await openPreferences(page);
    await expect(
      panel.getByText("Unavailable saved provider", { exact: true }),
    ).toBeVisible();
    await expect(
      panel.getByRole("switch", {
        name: "Use metadata providers",
        exact: true,
      }),
    ).toBeChecked();
    await panel
      .getByRole("switch", { name: "Use metadata providers", exact: true })
      .uncheck();
    stage = "disabled retained reference before explicit save";
    await savePreferences(page, {
      revision: 3,
      enabled: false,
      providers: [second],
    });
    stage = "disabled retained reference accepted before reload";
    panel = await openPreferences(page);
    await expect(
      panel.getByText("Unavailable saved provider", { exact: true }),
    ).toBeVisible();
    await expect(
      panel.getByRole("switch", {
        name: "Use metadata providers",
        exact: true,
      }),
    ).not.toBeChecked();
    expect(
      isDeepStrictEqual(await read("/api/v1/metadata/settings"), {
        ...defaults,
        revision: 4,
        enabled: false,
        providers: [second],
      }),
    ).toBe(true);

    stage = "owner complete music snapshot equality";
    const after = await preferencesLibraryState(page, read, saved);
    diagnostics = Object.fromEntries(
      (Object.keys(before) as (keyof typeof before)[]).map((key) => [
        key,
        isDeepStrictEqual(after[key], before[key]),
      ]),
    );
    expect(isDeepStrictEqual(after, before)).toBe(true);
    stage = "other account complete private snapshot equality";
    const otherAfter = {
      preferences: await otherRead("/api/v1/metadata/settings"),
      catalog: await otherRead("/api/v1/recordings"),
      playlists: await otherRead("/api/v1/playlists"),
      activity: await otherRead("/api/v1/activity"),
    };
    diagnostics = Object.fromEntries(
      (Object.keys(otherBefore) as (keyof typeof otherBefore)[]).map((key) => [
        key,
        isDeepStrictEqual(otherAfter[key], otherBefore[key]),
      ]),
    );
    expect(isDeepStrictEqual(otherAfter, otherBefore)).toBe(true);
    stage = "exact manifest-only provider traffic equality";
    diagnostics = {
      trafficUnchanged: isDeepStrictEqual(owned.calls, callsAfterApproval),
      total: owned.calls.total,
      firstManifests: owned.calls.manifest[0],
      secondManifests: owned.calls.manifest[1],
    };
    expect(isDeepStrictEqual(owned.calls, callsAfterApproval)).toBe(true);
    stage = "audio remains paused without a selected source";
    diagnostics = {};
    expect(
      await page
        .locator("audio[data-hito-player]")
        .evaluate(
          (element: HTMLAudioElement) =>
            element.paused && element.currentTime === 0 && !element.currentSrc,
        ),
    ).toBe(true);
    stage = "four accepted save workflow completions captured";
    diagnostics = { ...evidence.evidence };
    await expect.poll(() => evidence.evidence.acceptedSaves).toBe(4);
    await evidence.finish();
    const observed = evidence.evidence;
    stage = "bounded private workflow log and browser traffic counters";
    diagnostics = { ...observed };
    expect(
      !observed.failed &&
        observed.snapshots > 0 &&
        observed.started >= 4 &&
        observed.foreign +
          observed.audio +
          observed.download +
          observed.catalog +
          observed.sourceWrites +
          observed.pageErrors ===
          0 &&
        observed.requests <= 400,
    ).toBe(true);
    stage = "zero forbidden or unexpected provider calls";
    diagnostics = {
      descriptor: owned.calls.descriptor,
      profile: owned.calls.profile,
      audio: owned.calls.audio,
      download: owned.calls.download,
      unexpected: owned.calls.unexpected,
      unsafeHeaders: owned.calls.unsafeHeaders,
    };
    expect(
      owned.calls.descriptor +
        owned.calls.profile +
        owned.calls.audio +
        owned.calls.download +
        owned.calls.unexpected +
        owned.calls.unsafeHeaders,
    ).toBe(0);
  } catch {
    failed = true;
  } finally {
    if (!failed) {
      stage = "owned contexts, provider and console capture cleanup";
      diagnostics = { ...evidence.evidence };
    }
    const cleanup = await Promise.allSettled([
      context.close(),
      otherContext?.close() ?? Promise.resolve(),
    ]);
    if (cleanup.some((result) => result.status === "rejected")) failed = true;
    try {
      await provider?.close();
    } catch {
      failed = true;
    }
    await evidence.finish();
    if (evidence.evidence.failed) failed = true;
  }
  if (failed)
    throw new Error(
      `Metadata preferences browser verification failed at ${stage}; checks=${JSON.stringify(diagnostics)}; page_errors=${JSON.stringify(evidence.pageErrorDetails)}`,
    );
});
