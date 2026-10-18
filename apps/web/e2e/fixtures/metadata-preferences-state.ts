import { randomUUID } from "node:crypto";
import { isDeepStrictEqual } from "node:util";
import { expect, type Locator, type Page } from "@playwright/test";
import type { components } from "../../src/generated";

type Schema = components["schemas"];

export function preferencesReader(page: Page) {
  let reads = 0;
  return async <T>(path: string): Promise<T> => {
    if (++reads > 100)
      throw new Error("Preferences public read budget exceeded");
    const response = await page.request.get(path, { timeout: 10_000 });
    if (response.status() !== 200)
      throw new Error("Preferences public read failed");
    const body = await response.body();
    if (body.length > 524_288)
      throw new Error("Preferences public read exceeded its bound");
    return JSON.parse(body.toString()) as T;
  };
}
type Reader = ReturnType<typeof preferencesReader>;

export async function preferencesAction<T>(
  page: Page,
  control: Locator,
  method: "POST" | "PUT" | "DELETE",
  path: string,
  status: number,
  progress?: (
    phase: "accepted_response" | "bounded_body" | "no_content",
  ) => void,
) {
  const results = await Promise.allSettled([
    page.waitForResponse(
      (response) =>
        new URL(response.url()).pathname === path &&
        response.request().method() === method,
      { timeout: 15_000 },
    ),
    control.click(),
  ]);
  if (
    results[0].status !== "fulfilled" ||
    results[1].status !== "fulfilled" ||
    results[0].value.status() !== status
  )
    throw new Error(
      "The preferences browser action did not receive its accepted response",
    );
  const response = results[0].value;
  progress?.("accepted_response");
  if (response.status() === 204) {
    progress?.("no_content");
    return { body: null, sent: response.request().postDataJSON() as unknown };
  }
  const body = await response.body();
  if (body.length > 524_288)
    throw new Error("Preferences action exceeded its bound");
  progress?.("bounded_body");
  return {
    body: body.length ? (JSON.parse(body.toString()) as T) : null,
    sent: response.request().postDataJSON() as unknown,
  };
}

export async function openPreferences(page: Page) {
  await page.goto("/settings?section=metadata");
  await expect(
    page.getByRole("tab", { name: "Metadata", exact: true }),
  ).toHaveAttribute("aria-selected", "true");
  const panel = page.getByRole("region", {
    name: "Metadata preferences",
    exact: true,
  });
  await expect(
    panel.getByRole("checkbox", {
      name: "Use metadata providers",
      exact: true,
    }),
  ).toBeEnabled();
  return panel;
}

export async function savePreferences(
  page: Page,
  expected: Schema["UpdateMetadataSettings"],
) {
  const accepted = await preferencesAction<Schema["MetadataSettings"]>(
    page,
    page.getByRole("button", {
      name: "Save metadata preferences",
      exact: true,
    }),
    "PUT",
    "/api/v1/metadata/settings",
    200,
  );
  expect(isDeepStrictEqual(accepted.sent, expected)).toBe(true);
  expect(
    isDeepStrictEqual(accepted.body, {
      ...expected,
      revision: expected.revision + 1,
    }),
  ).toBe(true);
  await expect(
    page.getByRole("status").filter({ hasText: "Metadata preferences saved." }),
  ).toBeVisible();
}

export async function seedPreferencesLibrary(
  page: Page,
  read: Reader,
  user: Schema["User"],
) {
  await page.goto("/settings");
  for (const folder of ["Music", "Archive"]) {
    await page
      .getByRole("button", { name: "Add music folder", exact: true })
      .click();
    const picker = page.getByRole("dialog", {
      name: "Add a music folder",
      exact: true,
    });
    await picker
      .getByRole("button", { name: `Open ${folder}`, exact: true })
      .click();
    await picker
      .getByRole("button", { name: "Use this folder", exact: true })
      .click();
    await expect(picker).toBeHidden();
  }
  await expect(
    page.locator(".music-folder-row").filter({ hasText: "1 file read" }),
  ).toHaveCount(2, { timeout: 20_000 });
  const catalog = await read<Schema["RecordingPage"]>("/api/v1/recordings");
  expect(catalog.total === 2 && catalog.items.length === 2).toBe(true);
  const ids = catalog.items.map((recording) => recording.id);
  const playlist = randomUUID();
  const entries = [randomUUID(), randomUUID()];
  const privateValues = [
    `Private preferences title ${randomUUID()}`,
    `Private preferences artist ${randomUUID()}`,
    `Private preferences selection ${randomUUID()}`,
  ];
  const actions = [
    { kind: "playlist_create", playlist_id: playlist, name: privateValues[2] },
    ...entries.map((entry_id, position) => ({
      kind: "entry_insert" as const,
      playlist_id: playlist,
      entry_id,
      recording_id: ids[0],
      position,
    })),
    {
      kind: "recording_title_set",
      recording_id: ids[0],
      title: privateValues[0],
    },
    {
      kind: "recording_artist_set",
      recording_id: ids[0],
      artist: privateValues[1],
    },
    { kind: "like_set", recording_id: ids[0], liked: true },
    {
      kind: "progress_set",
      recording_id: ids[0],
      position_ms: 400,
      speed: 1.25,
    },
  ] satisfies Schema["Action"][];
  const batch = {
    device_id: "owned-preferences-browser",
    operations: actions.map((action) => ({ id: randomUUID(), action })),
  } satisfies Schema["SyncBatch"];
  const response = await page.request.post("/api/v1/sync", {
    headers: {
      "x-hito-request": "1",
      "x-hito-account": user.id,
      Origin: new URL(page.url()).origin,
    },
    data: batch,
    timeout: 10_000,
  });
  try {
    expect(response.status()).toBe(200);
    const body = await response.body();
    if (body.length > 4096)
      throw new Error("The preference seed acknowledgement exceeded its bound");
    const receipt = JSON.parse(body.toString()) as Schema["SyncReceipt"];
    expect(
      isDeepStrictEqual(
        receipt.acknowledged,
        batch.operations.map((operation) => operation.id),
      ),
    ).toBe(true);
  } finally {
    await response.dispose();
  }
  return { ids, playlist, entries, privateValues };
}

async function sharedOriginalSourceChoice(
  page: Page,
  recording: string,
): Promise<{ status: 404; choice: null }> {
  const response = await page.request.get(
    `/api/v1/recordings/${recording}/source-choice`,
    { timeout: 10_000 },
  );
  expect(response.status()).toBe(404);
  const body = await response.body();
  if (body.length > 4096)
    throw new Error("The original source choice response exceeded its bound");
  const unavailable = {
    error: "Resource not found",
  } satisfies Schema["ErrorResponse"];
  expect(isDeepStrictEqual(JSON.parse(body.toString()), unavailable)).toBe(
    true,
  );
  return { status: 404, choice: null };
}

export async function preferencesLibraryState(
  page: Page,
  read: Reader,
  saved: Awaited<ReturnType<typeof seedPreferencesLibrary>>,
) {
  const progress = await page.request.post("/api/v1/recordings/progress", {
    headers: { "x-hito-request": "1", Origin: new URL(page.url()).origin },
    data: { recording_ids: saved.ids } satisfies Schema["SavedPlaybackRequest"],
    timeout: 10_000,
  });
  let savedProgress: Schema["RecordingProgress"][];
  try {
    expect(progress.status()).toBe(200);
    const body = await progress.body();
    if (body.length > 524_288)
      throw new Error("The preferences progress response exceeded its bound");
    savedProgress = JSON.parse(
      body.toString(),
    ) as Schema["RecordingProgress"][];
  } finally {
    await progress.dispose();
  }
  return {
    libraries: await read<Schema["Library"][]>("/api/v1/libraries"),
    catalog: await read<Schema["RecordingPage"]>("/api/v1/recordings"),
    artists: await read<Schema["ArtistPage"]>("/api/v1/artists"),
    releases: await read<Schema["ReleasePage"]>("/api/v1/releases"),
    playlist: await read<Schema["PlaylistDetail"]>(
      `/api/v1/playlists/${saved.playlist}`,
    ),
    activity: await read<Schema["Activity"]>("/api/v1/activity"),
    events: await read<Schema["SyncPage"]>("/api/v1/sync?cursor=0"),
    progress: savedProgress,
    metadata: await Promise.all(
      saved.ids.map((id) =>
        read<Schema["RecordingMetadata"]>(`/api/v1/recordings/${id}/metadata`),
      ),
    ),
    sources: await Promise.all(
      saved.ids.map((id) =>
        read<Schema["Source"][]>(`/api/v1/recordings/${id}/sources`),
      ),
    ),
    choices: await Promise.all(
      saved.ids.map((id) => sharedOriginalSourceChoice(page, id)),
    ),
  };
}
