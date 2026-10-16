import { randomUUID } from "node:crypto";
import {
  expect,
  test,
  type APIRequestContext,
} from "/workspace/apps/web/node_modules/@playwright/test";
import type { components } from "../../../../apps/web/src/generated";

declare global {
  interface Window {
    captureSpotifyPlaylistHistory: (value: string) => Promise<void>;
  }
}

type Schema = components["schemas"];
const pluginAddress = "http://plugin-spotify:8092/";
const playlistReference = process.env.HITO_SPOTIFY_TEST_PLAYLIST?.trim() ?? "";
const trackCountInput = process.env.HITO_SPOTIFY_TEST_TRACKS?.trim() ?? "";

function object(value: unknown): Record<string, unknown> {
  if (value === null || typeof value !== "object" || Array.isArray(value))
    throw new Error("Expected a JSON object");
  return value as Record<string, unknown>;
}

function array(value: unknown): unknown[] {
  if (!Array.isArray(value)) throw new Error("Expected a JSON array");
  return value;
}

function text(value: unknown): string {
  if (typeof value !== "string") throw new Error("Expected JSON text");
  return value;
}

function consoleStrings(value: unknown): string[] {
  if (typeof value === "string") return [value];
  if (Array.isArray(value)) return value.flatMap(consoleStrings);
  if (value !== null && typeof value === "object")
    return Object.values(value).flatMap(consoleStrings);
  return [];
}

async function json<T>(response: { json(): Promise<unknown> }): Promise<T> {
  try {
    return (await response.json()) as T;
  } catch {
    throw new Error("A response did not contain valid JSON");
  }
}

async function detail(
  request: APIRequestContext,
  path: string,
): Promise<Schema["PlaylistDetail"]> {
  const response = await request.get(path);
  expect(response.status(), "Copied playlist HTTP status").toBe(200);
  return json<Schema["PlaylistDetail"]>(response);
}

test("a public Spotify link becomes a complete private editable metadata copy without OAuth", async ({
  page,
  context,
  playwright,
}, info) => {
  test.skip(
    !playlistReference,
    "Set HITO_SPOTIFY_TEST_PLAYLIST and its independently verified HITO_SPOTIFY_TEST_TRACKS count to run the live public Spotify browser case.",
  );
  let phase = "validating the public link and required independent HITO_SPOTIFY_TEST_TRACKS count";
  let listener: APIRequestContext | undefined;
  let browserErrors = 0;
  let audioRequests = 0;
  let providerBrowserRequests = 0;
  let authorizationRequests = 0;
  let importRequests = 0;
  const browserUrls: string[] = [];
  const history: string[] = [];
  const logs: Promise<string[]>[] = [];
  const adminPassword = randomUUID();
  const listenerPassword = randomUUID();
  const headers = {
    "x-hito-request": "1",
    origin: "http://localhost:8080",
  };
  page.on("pageerror", () => browserErrors++);
  page.on("request", (request) => {
    const url = new URL(request.url());
    browserUrls.push(request.url());
    if (request.resourceType() === "media" || /\/audio$/.test(url.pathname))
      audioRequests++;
    if (/(^|\.)spotify\.com$|(^|\.)scdn\.co$/.test(url.hostname))
      providerBrowserRequests++;
    if (/\/authorization(?:\/|$)/.test(url.pathname)) authorizationRequests++;
    if (
      request.method() === "POST" &&
      /\/plugins\/[^/]+\/playlists\/import$/.test(url.pathname)
    )
      importRequests++;
  });
  page.on("console", (message) => {
    logs.push(
      Promise.all(message.args().map((argument) => argument.jsonValue()))
        .then((values) => values.flatMap(consoleStrings))
        .catch(() => [message.text()]),
    );
  });

  try {
    const expectedCount = Number(trackCountInput);
    expect(
      Number.isSafeInteger(expectedCount) && expectedCount > 0,
      "HITO_SPOTIFY_TEST_TRACKS must contain an independently verified positive entry count",
    ).toBe(true);
    expect(
      /^https:\/\/open\.spotify\.com\/playlist\/[A-Za-z0-9]{22}(?:\?[^#]*)?$/.test(
        playlistReference,
      ),
      "The live input must be a public Spotify playlist share link",
    ).toBe(true);
    await page.exposeFunction("captureSpotifyPlaylistHistory", (value: string) => {
      history.push(value);
    });
    await page.addInitScript(() => {
      localStorage.setItem("hito-log-level", "debug");
      for (const method of ["pushState", "replaceState"] as const) {
        const original = window.history[method].bind(window.history);
        window.history[method] = (state, unused, url) => {
          void window.captureSpotifyPlaylistHistory(
            JSON.stringify({ url: String(url ?? location.href), state }),
          );
          original(state, unused, url);
        };
      }
    });

    phase = "setting up the temporary administrator";
    const setupResponse = await page.request.get("/api/v1/setup");
    expect(setupResponse.status()).toBe(200);
    const setup = await json<Schema["SetupStatus"]>(setupResponse);
    expect(setup.needs_setup, "The isolated server must have a fresh database").toBe(
      true,
    );
    await page.goto("/settings");
    await expect(
      page.getByRole("heading", { name: "Set up Hito", exact: true }),
    ).toBeVisible();
    await page.getByLabel("Username", { exact: true }).fill("spotify-e2e-admin");
    await page.getByLabel("Password", { exact: true }).fill(adminPassword);
    await page
      .getByRole("button", { name: "Create administrator", exact: true })
      .click();
    await expect(
      page.getByRole("heading", { name: "Your library", exact: true }),
    ).toBeVisible();

    phase = "offering the real Spotify service in Settings";
    await page.goto("/settings");
    await page.getByRole("tab", { name: "Plugins", exact: true }).click();
    await page.getByText("Services for this server", { exact: true }).click();
    await page
      .getByLabel("Service name", { exact: true })
      .fill("Spotify public playlists");
    await page.getByLabel("Service address", { exact: true }).fill(pluginAddress);
    const offeredResponse = page.waitForResponse(
      (response) =>
        new URL(response.url()).pathname === "/api/v1/plugins/services" &&
        response.request().method() === "POST",
    );
    await page.getByRole("button", { name: "Offer service", exact: true }).click();
    const offered = await offeredResponse;
    expect(offered.status()).toBe(201);
    const service = await json<Schema["PluginService"]>(offered);
    await expect(page.getByRole("combobox", { name: "Service", exact: true }))
      .toHaveValue(service.id);

    phase = "connecting the personal service with an empty token";
    const bearer = page.getByLabel("Bearer token (optional)", { exact: true });
    expect(await bearer.inputValue(), "The personal token field must start empty")
      .toBe("");
    const connectedResponse = page.waitForResponse(
      (response) =>
        new URL(response.url()).pathname === "/api/v1/plugins" &&
        response.request().method() === "POST",
    );
    await page.getByRole("button", { name: "Connect service", exact: true }).click();
    const connected = await connectedResponse;
    expect(connected.status()).toBe(201);
    const sentConnection = connected.request().postDataJSON() as Schema["InstallPlugin"];
    expect(sentConnection.bearer_token == null, "The connection must send no token")
      .toBe(true);
    const connection = await json<Schema["PluginInstallation"]>(connected);
    expect(connection.credential_set).toBe(false);
    expect(connection.manifest.id).toBe("org.hito.spotify");
    expect(connection.manifest.name).toBe("Spotify public playlists");
    expect(connection.manifest.capabilities).toEqual(["playlists"]);
    expect(connection.manifest.authorization == null, "The manifest must need no OAuth")
      .toBe(true);
    const plugin = page.getByRole("listitem").filter({
      has: page.getByRole("heading", {
        name: "Spotify public playlists",
        exact: true,
      }),
    });
    await expect(plugin.getByText("No token saved", { exact: true })).toBeVisible();

    phase = "importing the share link through Playlists";
    await page.goto("/playlists");
    const form = page.getByRole("region", { name: "Import a playlist" });
    await form
      .getByRole("combobox", { name: "Playlist service", exact: true })
      .selectOption(connection.id);
    await form
      .getByRole("textbox", { name: "Playlist link or identifier", exact: true })
      .fill(playlistReference);
    const importPath = `/api/v1/plugins/${connection.id}/playlists/import`;
    const importedResponse = page.waitForResponse(
      (response) =>
        new URL(response.url()).pathname === importPath &&
        response.request().method() === "POST",
      { timeout: 90000 },
    );
    await form.getByRole("button", { name: "Import a copy", exact: true }).click();
    const imported = await importedResponse;
    expect(imported.status(), "The live playlist import must succeed").toBe(200);
    const submitted = imported.request().postDataJSON() as Schema["ImportProviderPlaylist"];
    const receipt = await json<Schema["PlaylistImportReceipt"]>(imported);
    expect(receipt.request_id === submitted.request_id).toBe(true);
    expect(submitted.playlist_reference === playlistReference).toBe(true);
    expect(submitted.installation_revision).toBe(connection.revision);
    expect(Object.keys(receipt).sort()).toEqual(["playlist_id", "request_id"]);
    expect(/^[0-9a-f-]{36}$/.test(receipt.playlist_id)).toBe(true);
    const copyPath = `/playlists/${receipt.playlist_id}`;
    const detailPath = `/api/v1/playlists/${receipt.playlist_id}`;
    await expect.poll(() => new URL(page.url()).pathname === copyPath).toBe(true);
    const copied = await detail(page.request, detailPath);
    expect(copied.playlist.kind).toBe("local");
    expect(copied.entries.length).toBeGreaterThan(0);
    expect(copied.entries.length, "The copy must retain the independently verified full count")
      .toBe(expectedCount);
    const origin = copied.origin;
    if (!origin) throw new Error("The copied playlist must retain its attribution");
    expect(origin.provider).toBe("org.hito.spotify");
    expect(origin.installation_id === connection.id).toBe(true);
    expect(origin.attribution.label).toBe("Spotify");

    phase = "checking every ordered provider page and duplicate occurrence";
    const providerResponse = await page.request.post(
      `${pluginAddress}v1/playlists/detail`,
      { data: { kind: "canonical", playlist_id: origin.provider_playlist_id } },
    );
    expect(providerResponse.status(), "The live public provider detail must succeed")
      .toBe(200);
    const provider = object(await json<unknown>(providerResponse));
    expect(provider.total_count === copied.entries.length).toBe(true);
    expect(provider.snapshot === origin.snapshot).toBe(true);
    expect(provider.name === copied.playlist.name).toBe(true);
    const providerEntries: Record<string, unknown>[] = [];
    const cursors = new Set<string>();
    let cursor: string | null = null;
    let pageCount = 0;
    do {
      const response = await page.request.post(`${pluginAddress}v1/playlists/items`, {
        data: {
          playlist_id: origin.provider_playlist_id,
          snapshot: origin.snapshot,
          cursor,
          limit: 100,
        },
      });
      expect(response.status(), "Each retained provider page must be readable").toBe(200);
      const providerPage = object(await json<unknown>(response));
      expect(providerPage.playlist_id === origin.provider_playlist_id).toBe(true);
      expect(providerPage.snapshot === origin.snapshot).toBe(true);
      const entries = array(providerPage.items).map(object);
      expect(entries.length).toBeLessThanOrEqual(100);
      providerEntries.push(...entries);
      pageCount++;
      expect(pageCount, "Provider pagination must terminate within its bound")
        .toBeLessThanOrEqual(100);
      expect(providerEntries.length).toBeLessThanOrEqual(copied.entries.length);
      cursor = providerPage.next_cursor == null ? null : text(providerPage.next_cursor);
      if (cursor !== null) {
        expect(cursors.has(cursor), "Provider page cursors must not repeat").toBe(false);
        cursors.add(cursor);
      }
    } while (cursor !== null);
    expect(providerEntries.length).toBe(copied.entries.length);
    if (copied.entries.length > 100) expect(pageCount).toBeGreaterThan(1);
    expect(new Set(copied.entries.map((entry) => entry.entry_id)).size)
      .toBe(copied.entries.length);
    const recordings = new Map<string, string>();
    let duplicateOccurrenceCount = 0;
    const privateMetadata = new Set<string>([copied.playlist.name]);
    for (const [index, entry] of copied.entries.entries()) {
      const source = providerEntries[index];
      const item = object(source.item);
      expect(entry.position).toBe(index);
      expect(entry.available).toBe(false);
      expect(entry.origin?.occurrence_id === source.occurrence_id).toBe(true);
      expect(entry.origin?.kind === item.kind).toBe(true);
      privateMetadata.add(entry.title);
      if (item.kind === "recording") {
        const recording = object(item.recording);
        const providerId = text(recording.id);
        expect(entry.origin?.provider_recording_id === providerId).toBe(true);
        expect(entry.title === recording.title).toBe(true);
        expect(entry.artist === recording.artist).toBe(true);
        expect(typeof entry.recording_id === "string").toBe(true);
        if (!entry.recording_id) throw new Error("Known metadata needs a recording identity");
        const existing = recordings.get(providerId);
        if (existing !== undefined) {
          duplicateOccurrenceCount++;
          expect(existing === entry.recording_id, "Duplicates must share one recording")
            .toBe(true);
        }
        recordings.set(providerId, entry.recording_id);
      } else {
        expect(item.kind === "unavailable" || item.kind === "unknown").toBe(true);
        expect(entry.recording_id == null).toBe(true);
        expect(
          entry.origin?.provider_recording_id ===
            (item.provider_recording_id ?? null),
        ).toBe(true);
      }
    }
    expect(recordings.size, "The live selection must contain recording metadata")
      .toBeGreaterThan(0);

    phase = "checking unavailable playback and zero audio sources";
    const table = page.getByRole("table", { name: "Playlist entries", exact: true });
    const rows = table.locator("tbody").getByRole("row");
    await expect(rows).toHaveCount(copied.entries.length);
    await expect(page.getByRole("button", { name: "Play playlist", exact: true }))
      .toBeDisabled();
    const entryPlayback = await table.getByRole("button", { name: /^Play / }).all();
    expect(entryPlayback.length).toBeGreaterThan(0);
    expect((await Promise.all(entryPlayback.map((button) => button.isDisabled())))
      .every(Boolean)).toBe(true);
    const recordingIds = [...recordings.values()];
    let readySources = 0;
    let sourceCount = 0;
    for (let start = 0; start < recordingIds.length; start += 10) {
      const responses = await Promise.all(recordingIds.slice(start, start + 10)
        .map((id) => page.request.get(`/api/v1/recordings/${id}/sources`)));
      for (const response of responses) {
        expect(response.status()).toBe(200);
        const sources = await json<Schema["Source"][]>(response);
        sourceCount += sources.length;
        readySources += sources.filter((source) => source.ready).length;
      }
    }
    expect(sourceCount).toBe(0);
    expect(readySources).toBe(0);
    expect(await page.locator("audio").getAttribute("src")).toBeNull();

    phase = "checking account privacy through the public API";
    const created = await page.request.post("/api/v1/users", {
      headers,
      data: { username: "spotify-e2e-listener", password: listenerPassword },
    });
    expect(created.status()).toBe(201);
    listener = await playwright.request.newContext({ baseURL: "http://localhost:8080" });
    const signedIn = await listener.post("/api/v1/login", {
      headers,
      data: { username: "spotify-e2e-listener", password: listenerPassword },
    });
    expect(signedIn.status()).toBe(200);
    const privatePlugins = await listener.get("/api/v1/plugins");
    expect(privatePlugins.status()).toBe(200);
    const listenerPlugins = await json<Schema["PluginSettings"]>(privatePlugins);
    expect(listenerPlugins.installations.length).toBe(0);
    const privatePlaylists = await listener.get("/api/v1/playlists");
    expect(privatePlaylists.status()).toBe(200);
    expect((await json<Schema["Playlist"][]>(privatePlaylists)).length).toBe(0);
    expect((await listener.get(detailPath)).status()).toBe(404);
    const privateCatalog = await listener.get("/api/v1/recordings");
    expect(privateCatalog.status()).toBe(200);
    expect((await json<Schema["RecordingPage"]>(privateCatalog)).total).toBe(0);

    phase = "checking persistence after disconnecting the provider";
    await page.reload();
    await expect(rows).toHaveCount(copied.entries.length);
    expect(JSON.stringify(await detail(page.request, detailPath)) === JSON.stringify(copied))
      .toBe(true);
    await page.goto("/settings?section=plugins");
    await plugin.getByRole("button", { name: "Remove", exact: true }).click();
    const disconnectedResponse = page.waitForResponse(
      (response) =>
        new URL(response.url()).pathname === `/api/v1/plugins/${connection.id}` &&
        response.request().method() === "DELETE",
    );
    await plugin.getByRole("button", { name: "Remove plugin", exact: true }).click();
    expect((await disconnectedResponse).status()).toBe(204);
    await page.goto(copyPath);
    await expect(rows).toHaveCount(copied.entries.length);
    expect(JSON.stringify(await detail(page.request, detailPath)) === JSON.stringify(copied))
      .toBe(true);

    phase = "editing the copy and retaining the remaining order on reload";
    const removedResponse = page.waitForResponse(
      (response) =>
        new URL(response.url()).pathname === "/api/v1/sync" &&
        response.request().method() === "POST",
    );
    await rows.first().getByRole("button", { name: /^Remove / }).click();
    const removed = await removedResponse;
    expect(removed.status()).toBe(200);
    const edits = removed.request().postDataJSON() as Schema["SyncBatch"];
    expect(edits.operations.length).toBe(1);
    const action = edits.operations[0].action;
    expect(action.kind).toBe("entry_remove");
    expect(
      action.kind === "entry_remove" &&
        action.playlist_id === receipt.playlist_id &&
        action.entry_id === copied.entries[0].entry_id,
    ).toBe(true);
    await expect(rows).toHaveCount(copied.entries.length - 1);
    const edited = await detail(page.request, detailPath);
    expect(JSON.stringify(edited.entries) === JSON.stringify(copied.entries.slice(1)))
      .toBe(true);
    await page.reload();
    expect(JSON.stringify(await detail(page.request, detailPath)) === JSON.stringify(edited))
      .toBe(true);
    if (edited.entries.length) {
      await expect(rows).toHaveCount(edited.entries.length);
      const remainingPlayback = await table.getByRole("button", { name: /^Play / }).all();
      expect((await Promise.all(remainingPlayback.map((button) => button.isDisabled())))
        .every(Boolean)).toBe(true);
    }

    phase = "checking browser errors and private observations";
    const capturedLogs = (await Promise.all(logs)).flat();
    expect(capturedLogs.some((value) => value.includes("playlist_import.completed")))
      .toBe(true);
    const storage = await page.evaluate(() => ({
      local: Object.entries(localStorage),
      session: Object.entries(sessionStorage),
      state: window.history.state,
    }));
    const observations = JSON.stringify({ browserUrls, history, storage, logs: capturedLogs });
    const cookies = await context.cookies();
    for (const secret of [
      playlistReference,
      encodeURIComponent(playlistReference),
      origin.provider_playlist_id,
      adminPassword,
      listenerPassword,
      ...cookies.map((cookie) => cookie.value),
    ])
      expect(secret.length === 0 || !observations.includes(secret)).toBe(true);
    for (const title of privateMetadata) {
      const escaped = JSON.stringify(title).slice(1, -1);
      expect(capturedLogs.some((value) =>
        value.includes(title) || value.includes(escaped))).toBe(false);
    }
    expect(browserErrors).toBe(0);
    expect(audioRequests).toBe(0);
    expect(providerBrowserRequests).toBe(0);
    expect(authorizationRequests).toBe(0);
    expect(importRequests).toBe(1);
    await info.attach("spotify-public-acceptance-summary", {
      body: JSON.stringify({
        entryCount: copied.entries.length,
        independentCountVerified: true,
        duplicateOccurrenceCount,
        entriesAfterEdit: edited.entries.length,
        durabilityChecks: ["browser_reload", "plugin_disconnect"],
      }, null, 2),
      contentType: "application/json",
    });
  } catch {
    throw new Error(
      `Spotify browser acceptance failed while ${phase}. Details stay in the ignored local Playwright trace.`,
    );
  } finally {
    await listener?.dispose();
  }
});
