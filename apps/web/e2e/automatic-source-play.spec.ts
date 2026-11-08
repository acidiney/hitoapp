import { randomUUID } from "node:crypto";
import { openPlaylistImport } from "./fixtures/playlist-dialogs";
import { expect, test as baseTest, type Page } from "@playwright/test";
import { automaticSourceFixture } from "./fixtures/automatic-source";
import type { Schema } from "../src/shared/schema";

const optedIn = process.env.HITO_AUTOMATIC_SOURCE_E2E === "1";
const test = baseTest.extend<{ automaticSourcePrivacy: void }>({
  automaticSourcePrivacy: [
    async ({}, provide) => {
      if (!optedIn) {
        await provide();
        return;
      }
      const previous = process.env.PLAYWRIGHT_NO_COPY_PROMPT;
      process.env.PLAYWRIGHT_NO_COPY_PROMPT = "1";
      try {
        await provide();
      } finally {
        if (previous === undefined)
          delete process.env.PLAYWRIGHT_NO_COPY_PROMPT;
        else process.env.PLAYWRIGHT_NO_COPY_PROMPT = previous;
      }
    },
    { auto: true },
  ],
});

test.use({ trace: "off", screenshot: "off", video: "off" });
test.skip(
  !optedIn,
  "Requires an opted-in fresh disposable automatic source server",
);

type Phase =
  | "configuration"
  | "setup"
  | "provider_fixture"
  | "connections"
  | "playlist_import"
  | "imported_identity"
  | "private_metadata"
  | "playlist_play_admission"
  | "actual_playback"
  | "automatic_match"
  | "preserved_collection"
  | "privacy_and_protocol"
  | "provider_cleanup"
  | "browser_cleanup"
  | "browser_callback";

async function read<T>(page: Page, path: string): Promise<T> {
  const response = await page.request.get(path);
  expect(response.status()).toBe(200);
  return response.json();
}

function retained(detail: Schema<"PlaylistDetail">) {
  return {
    ...detail,
    entries: detail.entries.map((entry) => ({ ...entry, available: false })),
  };
}

test("ordinary playlist Play automatically chooses studio audio for imported unavailable recordings without changing the saved collection", async ({
  page,
  context,
  baseURL,
}) => {
  test.setTimeout(90_000);
  let fixture: Awaited<ReturnType<typeof automaticSourceFixture>> | null = null;
  let phase: Phase = "configuration";
  let failedPhase: Phase | null = null;
  let pageErrors = 0;
  let externalRequests = 0;
  let manualWrites = 0;
  let adoptions = 0;
  let downloadWrites = 0;
  let browserAudioRequests = 0;
  let browserAudioResponses = 0;
  let invalidAudioResponses = 0;
  let privateLogs = 0;
  let callbackFailures = 0;
  let cleanupFailures = 0;
  const personalTitle = "My private listening label";
  const personalArtist = "My private grouping";

  try {
    if (!baseURL || process.env.PLAYWRIGHT_NO_COPY_PROMPT !== "1")
      throw new Error(
        "The isolated automatic source privacy configuration is required",
      );
    const configured = new URL(baseURL);
    if (
      configured.protocol !== "http:" ||
      !["localhost", "127.0.0.1", "e2e-server"].includes(configured.hostname) ||
      configured.port !== "8080" ||
      configured.username ||
      configured.password ||
      configured.search ||
      configured.hash
    )
      throw new Error("The automatic source server must be isolated locally");
    const origin = configured.origin;
    page.setDefaultTimeout(10_000);
    page.setDefaultNavigationTimeout(10_000);
    page.on("pageerror", () => pageErrors++);
    context.on("request", (request) => {
      try {
        const target = new URL(request.url());
        if (target.protocol.startsWith("http") && target.origin !== origin)
          externalRequests++;
        if (target.origin !== origin) return;
        if (/^\/api\/v1\/sources\/[0-9a-f-]+\/audio$/.test(target.pathname))
          browserAudioRequests++;
        if (["GET", "HEAD"].includes(request.method())) return;
        if (/\/sources\/(review|attach)$/.test(target.pathname)) manualWrites++;
        if (/\/plugins\/[^/]+\/recordings$/.test(target.pathname)) adoptions++;
        if (/\/download$/.test(target.pathname)) downloadWrites++;
      } catch {
        callbackFailures++;
      }
    });
    context.on("response", (response) => {
      try {
        const target = new URL(response.url());
        if (
          target.origin !== origin ||
          !/^\/api\/v1\/sources\/[0-9a-f-]+\/audio$/.test(target.pathname)
        )
          return;
        if (
          [200, 206].includes(response.status()) &&
          response.headers()["content-type"]?.startsWith("audio/wav")
        )
          browserAudioResponses++;
        else invalidAudioResponses++;
      } catch {
        callbackFailures++;
      }
    });

    phase = "setup";
    await page.goto("/library");
    await page
      .getByLabel("Username", { exact: true })
      .waitFor({ state: "visible" });
    await page
      .getByLabel("Password", { exact: true })
      .waitFor({ state: "visible" });
    const setup = page.getByRole("button", {
      name: "Create administrator",
      exact: true,
    });
    if (!(await setup.isVisible()))
      throw new Error("An uninitialized disposable server is required");
    await page.getByLabel("Username", { exact: true }).fill("admin");
    await page
      .getByLabel("Password", { exact: true })
      .fill("docker browser test password");
    await setup.click();
    await expect(
      page.getByRole("heading", { name: "Your library", exact: true }),
    ).toBeVisible();

    phase = "provider_fixture";
    const provider = await automaticSourceFixture();
    fixture = provider;
    page.on("console", (message) => {
      try {
        if (
          [...provider.privateValues, personalTitle, personalArtist].some(
            (value) => message.text().includes(value),
          )
        )
          privateLogs++;
      } catch {
        callbackFailures++;
      }
    });

    phase = "connections";
    const headers = { "x-hito-request": "1", origin };
    const connections: Schema<"PluginInstallation">[] = [];
    for (const [name, address] of [
      ["Owned metadata fixture", provider.metadataAddress],
      ["Owned audio fixture", provider.audioAddress],
    ]) {
      const offered = await page.request.post("/api/v1/plugins/services", {
        headers,
        data: {
          name,
          base_url: address,
        } satisfies Schema<"OfferPluginService">,
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
      connections.push(await installed.json());
    }

    phase = "playlist_import";
    await page.goto("/playlists");
    await openPlaylistImport(page);
    const form = page.getByRole("region", {
      name: "Import a playlist",
      exact: true,
    });
    await form
      .getByRole("combobox", { name: "Playlist service", exact: true })
      .selectOption(connections[0].id);
    await form
      .getByRole("textbox", {
        name: "Playlist link or identifier",
        exact: true,
      })
      .fill(provider.reference);
    const importing = page
      .waitForResponse(
        (response) =>
          new URL(response.url()).pathname ===
            `/api/v1/plugins/${connections[0].id}/playlists/import` &&
          response.request().method() === "POST",
      )
      .catch(() => null);
    await form
      .getByRole("button", { name: "Import a copy", exact: true })
      .click();
    const imported = await importing;
    if (imported === null) throw new Error("The import response must arrive");
    expect(imported.status()).toBe(200);
    const receipt: Schema<"PlaylistImportReceipt"> = await imported.json();
    await expect
      .poll(
        () =>
          new URL(page.url()).pathname === `/playlists/${receipt.playlist_id}`,
      )
      .toBe(true);

    phase = "imported_identity";
    const importedDetail = await read<Schema<"PlaylistDetail">>(
      page,
      `/api/v1/playlists/${receipt.playlist_id}`,
    );
    const recording = importedDetail.entries[0]?.recording_id;
    if (!recording)
      throw new Error("Known imported metadata must have a saved identity");
    expect(
      importedDetail.entries.length === 2 &&
        importedDetail.entries.every(
          (entry) => entry.recording_id === recording && !entry.available,
        ) &&
        new Set(importedDetail.entries.map((entry) => entry.entry_id)).size ===
          2,
    ).toBe(true);
    const initialSources = await read<Schema<"Source">[]>(
      page,
      `/api/v1/recordings/${recording}/sources`,
    );
    expect(initialSources.length).toBe(0);
    expect(provider.counts.audio).toBe(0);
    expect(browserAudioRequests).toBe(0);
    expect(await page.locator("audio").getAttribute("src")).toBeNull();
    expect(
      await page
        .locator("audio")
        .evaluate((element: HTMLAudioElement) => element.paused),
    ).toBe(true);
    expect(
      provider.counts.protocol +
        provider.counts.unexpected +
        provider.counts.handler,
    ).toBe(0);

    phase = "private_metadata";
    const edited = await page.request.post("/api/v1/sync", {
      headers,
      data: {
        device_id: "automatic-source-e2e",
        operations: [
          {
            id: randomUUID(),
            action: {
              kind: "recording_title_set",
              recording_id: recording,
              title: personalTitle,
            },
          },
          {
            id: randomUUID(),
            action: {
              kind: "recording_artist_set",
              recording_id: recording,
              artist: personalArtist,
            },
          },
          {
            id: randomUUID(),
            action: { kind: "like_set", recording_id: recording, liked: true },
          },
        ],
      } satisfies Schema<"SyncBatch">,
    });
    expect(edited.status()).toBe(200);
    await page.reload();
    const before = await read<Schema<"PlaylistDetail">>(
      page,
      `/api/v1/playlists/${receipt.playlist_id}`,
    );
    const metadataBefore = await read<Schema<"RecordingMetadata">>(
      page,
      `/api/v1/recordings/${recording}/metadata`,
    );
    expect(
      metadataBefore.selected_observation?.provider_recording_id ===
        provider.original.id &&
        metadataBefore.title.personal === personalTitle &&
        metadataBefore.artist.personal === personalArtist &&
        before.entries.every((entry) => entry.liked),
    ).toBe(true);
    const initialCatalog = await read<Schema<"RecordingPage">>(
      page,
      "/api/v1/recordings?limit=100",
    );
    const library = initialCatalog.items.find(
      (item) => item.id === recording,
    )?.library_id;
    if (!library)
      throw new Error(
        "The saved recording must have a private provider library",
      );
    expect(provider.counts.audio + browserAudioRequests).toBe(0);
    expect(await page.locator("audio").getAttribute("src")).toBeNull();
    expect(
      await page
        .locator("audio")
        .evaluate((element: HTMLAudioElement) => element.paused),
    ).toBe(true);

    phase = "playlist_play_admission";
    const play = page.getByRole("button", {
      name: "Play playlist",
      exact: true,
    });
    await expect(play).toBeEnabled();
    await play.click();

    phase = "actual_playback";
    const audio = page.locator("audio");
    await expect
      .poll(
        () =>
          audio.evaluate(
            (element: HTMLAudioElement) =>
              element.currentTime > 0.25 &&
              !element.paused &&
              element.error === null,
          ),
        { timeout: 20_000 },
      )
      .toBe(true);
    const playing = new URL(
      await audio.evaluate((element: HTMLAudioElement) => element.src),
    );
    expect(
      playing.origin === origin &&
        /^\/api\/v1\/sources\/[a-f0-9-]+\/audio$/.test(playing.pathname),
    ).toBe(true);
    expect(browserAudioResponses).toBeGreaterThan(0);
    expect(invalidAudioResponses).toBe(0);

    phase = "automatic_match";
    expect(provider.counts.search).toBeGreaterThan(0);
    expect(
      provider.queriesUseMusicalIdentity([personalTitle, personalArtist]),
    ).toBe(true);
    expect(provider.counts.audioGet).toBeGreaterThan(0);
    expect(provider.counts.unsuitableAudio).toBe(0);

    phase = "preserved_collection";
    const after = await read<Schema<"PlaylistDetail">>(
      page,
      `/api/v1/playlists/${receipt.playlist_id}`,
    );
    expect(
      JSON.stringify(retained(after)) === JSON.stringify(retained(before)) &&
        after.entries.every((entry) => entry.available),
    ).toBe(true);
    const metadataAfter = await read<Schema<"RecordingMetadata">>(
      page,
      `/api/v1/recordings/${recording}/metadata`,
    );
    const added = metadataAfter.observations.items.filter(
      (item) => item.plugin_installation_id === connections[1].id,
    );
    expect(
      JSON.stringify(metadataAfter.title) ===
        JSON.stringify(metadataBefore.title) &&
        JSON.stringify(metadataAfter.artist) ===
          JSON.stringify(metadataBefore.artist) &&
        JSON.stringify(metadataAfter.selected_observation) ===
          JSON.stringify(metadataBefore.selected_observation) &&
        added.length === 1 &&
        added[0].provider_recording_id === provider.suitable.id,
    ).toBe(true);
    const catalog = await read<Schema<"RecordingPage">>(
      page,
      `/api/v1/recordings?library=${encodeURIComponent(library)}`,
    );
    expect(
      catalog.items.length === 1 && catalog.items[0].id === recording,
    ).toBe(true);
    const resolvedSources = await read<Schema<"Source">[]>(
      page,
      `/api/v1/recordings/${recording}/sources`,
    );
    expect(
      resolvedSources.length > 0 &&
        resolvedSources.every((source) => source.provider === "plugin"),
    ).toBe(true);
    expect(
      (
        await page.request.get(`/api/v1/recordings/${recording}/download`)
      ).status(),
    ).toBe(404);
    expect(
      (
        await page.request.get(
          `/api/v1/playlists/${receipt.playlist_id}/download`,
        )
      ).status(),
    ).toBe(404);

    phase = "privacy_and_protocol";
    expect(
      provider.counts.protocol +
        provider.counts.unexpected +
        provider.counts.handler,
    ).toBe(0);
    expect(manualWrites + adoptions + downloadWrites + externalRequests).toBe(
      0,
    );
    expect(pageErrors + privateLogs + callbackFailures).toBe(0);
    await expect(
      page.getByRole("dialog", { name: "Review source match", exact: true }),
    ).toHaveCount(0);
  } catch {
    failedPhase = phase;
  } finally {
    try {
      if (!page.isClosed()) await page.close({ runBeforeUnload: false });
    } catch {
      cleanupFailures++;
    }
    try {
      await context.close();
    } catch {
      cleanupFailures++;
    }
    try {
      await fixture?.close();
    } catch {
      cleanupFailures++;
      failedPhase ??= "provider_cleanup";
    }
    if (callbackFailures > 0) failedPhase ??= "browser_callback";
    if (cleanupFailures > 0) failedPhase ??= "browser_cleanup";
    if (pageErrors + privateLogs > 0) failedPhase ??= "privacy_and_protocol";
    if (
      fixture &&
      fixture.counts.protocol +
        fixture.counts.unexpected +
        fixture.counts.handler >
        0
    )
      failedPhase ??= "privacy_and_protocol";
  }
  if (failedPhase !== null) {
    const failure = new Error(
      `Automatic source check failed at phase ${failedPhase}`,
    );
    failure.stack = failure.message;
    throw failure;
  }
});
