import { randomUUID } from "node:crypto";
import { isDeepStrictEqual } from "node:util";
import { expect, type Page } from "@playwright/test";
import type { components } from "../../src/generated";
import { reviewPluginInstallation } from "./plugin-installation-review";
import {
  openPreferences,
  preferencesAction,
  seedPreferencesLibrary,
  preferencesLibraryState,
  savePreferences,
} from "./metadata-preferences-state";
import {
  artistReviewOriginals,
  artistReviewDownloadAbsence,
} from "./artist-review-state";
import type { OutageProvider, OutageTrack } from "./providerOutageProvider";

type Schema = components["schemas"];
export type OutageSetupPhase =
  | "administrator"
  | "preferences"
  | "local_library"
  | "plugin_offer"
  | "plugin_review"
  | "plugin_connect"
  | "import_retry"
  | "import_queue"
  | "import_preserved"
  | "imported_identities"
  | "private_changes"
  | "healthy_resolution"
  | "preserved_resolution"
  | "manual_choice"
  | "empty_resolution"
  | "snapshot";
export type OutageResolutionDiagnostic = {
  target: "healthy" | "preserved" | "retry";
  route: "resolve";
  http_status?: number;
  outcome?: Schema["SourceResolutionOutcome"];
  sources_count?: number;
  ready_sources_count?: number;
};
const resolutionOutcomes = new Set<Schema["SourceResolutionOutcome"]>([
  "ready",
  "no_suitable_source",
  "provider_unavailable",
  "protected_choice",
  "cancelled",
]);
const importPhases = {
  retry: "import_retry",
  queue: "import_queue",
  preserved: "import_preserved",
} as const satisfies Record<string, OutageSetupPhase>;

export function outageReader(page: Page) {
  let count = 0;
  return async <T>(path: string): Promise<T> => {
    if (++count > 240) throw new Error("Owned public read budget exceeded");
    const response = await page.request.get(path, { timeout: 10_000 });
    try {
      const bytes = await response.body();
      if (response.status() !== 200 || bytes.length > 524_288)
        throw new Error("Owned public read rejected");
      return JSON.parse(bytes.toString()) as T;
    } finally {
      await response.dispose();
    }
  };
}
export type OutageReader = ReturnType<typeof outageReader>;

export async function outageWrite<T>(
  page: Page,
  account: string,
  method: "post" | "put",
  path: string,
  data: unknown,
  observeStatus?: (status: number) => void,
): Promise<T> {
  const response = await page.request[method](path, {
    headers: {
      "x-hito-request": "1",
      "x-hito-account": account,
      Origin: new URL(page.url()).origin,
    },
    data,
    timeout: 10_000,
  });
  try {
    observeStatus?.(response.status());
    const bytes = await response.body();
    if (response.status() !== 200 || bytes.length > 524_288)
      throw new Error("Owned public write rejected");
    return JSON.parse(bytes.toString()) as T;
  } finally {
    await response.dispose();
  }
}

export async function prepareOutageLibrary(
  page: Page,
  read: OutageReader,
  provider: OutageProvider,
  password: string,
  checkpoint: (phase: OutageSetupPhase) => void,
  observeResolution: (diagnostic: OutageResolutionDiagnostic) => void,
) {
  const recordResolution = (
    target: OutageResolutionDiagnostic["target"],
    result: Schema["SourceResolution"],
  ) =>
    observeResolution({
      target,
      route: "resolve",
      ...(resolutionOutcomes.has(result.outcome)
        ? { outcome: result.outcome }
        : {}),
      sources_count: result.sources.length,
      ready_sources_count: result.sources.filter((source) => source.ready)
        .length,
    });
  checkpoint("administrator");
  await page.goto("/");
  await page.getByLabel("Username", { exact: true }).fill("admin");
  await page.getByLabel("Password", { exact: true }).fill(password);
  await page
    .getByRole("button", { name: "Create administrator", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "Your library", exact: true }),
  ).toBeVisible();
  const user = await read<Schema["User"]>("/api/v1/me");
  checkpoint("preferences");
  const settings = await read<Schema["MetadataSettings"]>(
    "/api/v1/metadata/settings",
  );
  const preferences = await openPreferences(page);
  await preferences
    .getByRole("checkbox", { name: "Use metadata providers", exact: true })
    .uncheck();
  await savePreferences(page, { ...settings, enabled: false });
  checkpoint("local_library");
  const local = await seedPreferencesLibrary(page, read, user);
  checkpoint("plugin_offer");
  await page.getByRole("tab", { name: "Plugins", exact: true }).click();
  await page.getByText("Services for this server", { exact: true }).click();
  await page
    .getByLabel("Service address", { exact: true })
    .fill(provider.address);
  const offered = await preferencesAction<Schema["PluginService"]>(
    page,
    page.getByRole("button", { name: "Offer service", exact: true }),
    "POST",
    "/api/v1/plugins/services",
    201,
  );
  if (
    !offered.body ||
    !isDeepStrictEqual(offered.sent, { base_url: provider.address }) ||
    offered.body.name !== provider.manifest.name
  )
    throw new Error("Owned provider declaration mismatch");
  checkpoint("plugin_review");
  await page
    .getByRole("combobox", { name: "Service", exact: true })
    .selectOption(offered.body.id);
  await page
    .getByLabel("Bearer token (optional)", { exact: true })
    .fill(provider.token);
  const preview = await reviewPluginInstallation(
    page,
    offered.body,
    provider.manifest.name,
  );
  checkpoint("plugin_connect");
  const accepted = await preferencesAction<Schema["PluginInstallation"]>(
    page,
    page.getByRole("button", { name: "Accept and connect", exact: true }),
    "POST",
    "/api/v1/plugins",
    201,
  );
  if (
    !accepted.body ||
    !accepted.body.credential_set ||
    !isDeepStrictEqual(accepted.body.manifest, provider.manifest) ||
    !isDeepStrictEqual(accepted.sent, {
      ...preview.sent,
      accept_manifest_digest: preview.body.digest,
    })
  )
    throw new Error("Owned provider approval mismatch");
  const connection = accepted.body;
  const copies: Record<string, Schema["PlaylistDetail"]> = {};
  for (const key of ["retry", "queue", "preserved"] as const) {
    checkpoint(importPhases[key]);
    await page
      .getByRole("navigation", { name: "Main navigation", exact: true })
      .getByRole("link", { name: "Playlists", exact: true })
      .click();
    const form = page.getByRole("region", {
      name: "Import a playlist",
      exact: true,
    });
    await form
      .getByRole("combobox", { name: "Playlist service", exact: true })
      .selectOption(connection.id);
    await form
      .getByRole("textbox", {
        name: "Playlist link or identifier",
        exact: true,
      })
      .fill(key);
    const imported = await preferencesAction<Schema["PlaylistImportReceipt"]>(
      page,
      form.getByRole("button", { name: "Import a copy", exact: true }),
      "POST",
      `/api/v1/plugins/${connection.id}/playlists/import`,
      200,
    );
    if (!imported.body) throw new Error("Owned playlist import missing");
    const sent = imported.sent as Schema["ImportProviderPlaylist"];
    expect(
      sent.playlist_reference === key &&
        sent.installation_revision === connection.revision &&
        sent.request_id === imported.body.request_id,
    ).toBe(true);
    await page.waitForURL(
      new RegExp(`/playlists/${imported.body.playlist_id}$`),
    );
    copies[key] = await read<Schema["PlaylistDetail"]>(
      `/api/v1/playlists/${imported.body.playlist_id}`,
    );
    expect(copies[key].entries.every((entry) => !entry.available)).toBe(true);
  }
  checkpoint("imported_identities");
  const ids = Object.fromEntries(
    (Object.keys(provider.recordings) as OutageTrack[]).map((key) => {
      const entry = Object.values(copies)
        .flatMap((copy) => copy.entries)
        .find(
          (entry) =>
            entry.origin?.kind === "recording" &&
            entry.origin.provider_recording_id === provider.recordings[key].id,
        );
      if (!entry?.recording_id)
        throw new Error("Owned imported identity missing");
      return [key, entry.recording_id];
    }),
  ) as Record<OutageTrack, string>;
  expect(
    new Set(Object.values(ids)).size === 4 &&
      copies.preserved.entries.length === 2 &&
      copies.queue.entries.length === 3 &&
      copies.queue.entries[0].recording_id ===
        copies.queue.entries[2].recording_id &&
      new Set(copies.queue.entries.map((entry) => entry.entry_id)).size === 3 &&
      new Set(copies.preserved.entries.map((entry) => entry.entry_id)).size ===
        2,
  ).toBe(true);
  checkpoint("private_changes");
  const privateTitle = `Owned preserved title ${randomUUID()}`;
  const privateArtist = `Owned preserved artist ${randomUUID()}`;
  const actions = [
    {
      kind: "recording_title_set",
      recording_id: ids.preserved,
      title: privateTitle,
    },
    {
      kind: "recording_artist_set",
      recording_id: ids.preserved,
      artist: privateArtist,
    },
    { kind: "like_set", recording_id: ids.preserved, liked: true },
    {
      kind: "progress_set",
      recording_id: ids.preserved,
      position_ms: 431,
      speed: 1.25,
    },
  ] satisfies Schema["Action"][];
  const batch = {
    device_id: "owned-provider-outage-seed",
    operations: actions.map((action) => ({ id: randomUUID(), action })),
  } satisfies Schema["SyncBatch"];
  const receipt = await outageWrite<Schema["SyncReceipt"]>(
    page,
    user.id,
    "post",
    "/api/v1/sync",
    batch,
  );
  expect(
    isDeepStrictEqual(
      receipt.acknowledged,
      batch.operations.map((operation) => operation.id),
    ),
  ).toBe(true);
  const legacyResolutions: {
    recording: string;
    request: Schema["ResolveSource"];
    receipt: Schema["SourceResolution"];
  }[] = [];
  for (const key of ["healthy", "preserved"] as const) {
    checkpoint(
      key === "healthy" ? "healthy_resolution" : "preserved_resolution",
    );
    const request = {
      request_id: randomUUID(),
    } satisfies Schema["ResolveSource"];
    const result = await outageWrite<Schema["SourceResolution"]>(
      page,
      user.id,
      "post",
      `/api/v1/recordings/${ids[key]}/sources/resolve`,
      request,
      (status) =>
        observeResolution({
          target: key,
          route: "resolve",
          http_status: status,
        }),
    );
    recordResolution(key, result);
    legacyResolutions.push({ recording: ids[key], request, receipt: result });
    expect(
      result.outcome === "ready" &&
        result.sources.length === 1 &&
        result.sources[0].ready,
    ).toBe(true);
    if (key === "preserved") {
      checkpoint("manual_choice");
      await outageWrite<Schema["SourceChoiceReceipt"]>(
        page,
        user.id,
        "post",
        `/api/v1/recordings/${ids[key]}/source-choice`,
        {
          request_id: randomUUID(),
          source_id: result.sources[0].id,
        } satisfies Schema["ChooseSource"],
      );
      const choice = await read<Schema["SourceChoice"]>(
        `/api/v1/recordings/${ids[key]}/source-choice`,
      );
      expect(
        choice.mode === "manual" &&
          choice.revision === 1 &&
          choice.sources.some((source) => source.selected),
      ).toBe(true);
    }
  }
  checkpoint("empty_resolution");
  const oldRequest = {
    request_id: randomUUID(),
  } satisfies Schema["ResolveSource"];
  const oldReceipt = await outageWrite<Schema["SourceResolution"]>(
    page,
    user.id,
    "post",
    `/api/v1/recordings/${ids.retry}/sources/resolve`,
    oldRequest,
    (status) =>
      observeResolution({
        target: "retry",
        route: "resolve",
        http_status: status,
      }),
  );
  recordResolution("retry", oldReceipt);
  expect(
    oldReceipt.outcome === "no_suitable_source" &&
      oldReceipt.sources.length === 0,
  ).toBe(true);
  legacyResolutions.push({
    recording: ids.retry,
    request: oldRequest,
    receipt: oldReceipt,
  });
  return {
    user,
    local,
    copies,
    ids,
    connection,
    legacyResolutions,
    privateValues: [
      ...local.privateValues,
      privateTitle,
      privateArtist,
      "Fixture 440 Hz",
      "Fixture 660 Hz",
      "Hito test fixtures",
      "/music/Music/Fixture 440 Hz.wav",
      "/music/Archive/Fixture 660 Hz.wav",
      ...Object.values(provider.playlists).map((playlist) => playlist.name),
    ],
  };
}

export type OutageLibrary = Awaited<ReturnType<typeof prepareOutageLibrary>>;

export async function outageLibraryState(
  page: Page,
  read: OutageReader,
  library: OutageLibrary,
) {
  const ids = Object.values(library.ids);
  return {
    local: await preferencesLibraryState(page, read, library.local),
    originals: await artistReviewOriginals(),
    copies: await Promise.all(
      Object.values(library.copies).map((copy) =>
        read<Schema["PlaylistDetail"]>(`/api/v1/playlists/${copy.playlist.id}`),
      ),
    ),
    metadata: await Promise.all(
      ids.map((id) =>
        read<Schema["RecordingMetadata"]>(`/api/v1/recordings/${id}/metadata`),
      ),
    ),
    choices: await Promise.all(
      ids.map((id) =>
        read<Schema["SourceChoice"]>(`/api/v1/recordings/${id}/source-choice`),
      ),
    ),
    progress: await outageWrite<Schema["RecordingProgress"][]>(
      page,
      library.user.id,
      "post",
      "/api/v1/recordings/progress",
      { recording_ids: ids } satisfies Schema["SavedPlaybackRequest"],
    ),
    absent: await artistReviewDownloadAbsence(page, [
      ...library.local.ids,
      ...ids,
    ]),
    settings: await read<Schema["MetadataSettings"]>(
      "/api/v1/metadata/settings",
    ),
  };
}
export type OutageState = Awaited<ReturnType<typeof outageLibraryState>>;

export function assertOutagePreserved(
  before: OutageState,
  after: OutageState,
  library: OutageLibrary,
) {
  const same = (left: unknown, right: unknown) =>
    expect(isDeepStrictEqual(left, right)).toBe(true);
  same(before.originals, after.originals);
  same(before.local.playlist, after.local.playlist);
  same(before.local.metadata, after.local.metadata);
  same(before.local.sources, after.local.sources);
  same(before.local.choices, after.local.choices);
  same(before.local.progress, after.local.progress);
  same(before.settings, after.settings);
  same(before.absent, after.absent);
  const unchanged = library.ids.preserved;
  same(
    before.metadata.find((item) => item.recording_id === unchanged),
    after.metadata.find((item) => item.recording_id === unchanged),
  );
  same(
    before.choices.find((item) => item.recording_id === unchanged),
    after.choices.find((item) => item.recording_id === unchanged),
  );
  same(
    before.progress.find((item) => item.recording_id === unchanged),
    after.progress.find((item) => item.recording_id === unchanged),
  );
  const entries = (state: OutageState) =>
    state.copies.map((copy) => ({
      ...copy,
      entries: copy.entries.map((entry) => ({ ...entry, available: false })),
    }));
  same(entries(before), entries(after));
  for (const metadata of before.metadata) {
    const current = after.metadata.find(
      (item) => item.recording_id === metadata.recording_id,
    );
    same(
      [metadata.title, metadata.artist, metadata.selected_observation],
      [current?.title, current?.artist, current?.selected_observation],
    );
  }
  same(
    before.local.events.operations,
    after.local.events.operations.slice(
      0,
      before.local.events.operations.length,
    ),
  );
  const heard = new Set([library.ids.retry, library.ids.healthy]);
  for (const event of after.local.events.operations.slice(
    before.local.events.operations.length,
  ))
    expect(
      "operation" in event &&
        event.operation.action.kind === "progress_set" &&
        heard.has(event.operation.action.recording_id),
    ).toBe(true);
  same(
    before.progress.find((item) => item.recording_id === library.ids.queue),
    after.progress.find((item) => item.recording_id === library.ids.queue),
  );
}
