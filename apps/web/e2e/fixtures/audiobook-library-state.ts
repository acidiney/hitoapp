import { randomUUID } from "node:crypto";
import { stat } from "node:fs/promises";
import { isDeepStrictEqual } from "node:util";
import { expect, type Page } from "@playwright/test";
import type { components } from "../../src/generated";
import { artistReviewOriginals } from "./artist-review-state";
import { seedPreferencesLibrary } from "./metadata-preferences-state";
import {
  libraryClient,
  type LibraryBudget,
  type LibraryClient,
  type Remember,
} from "./audiobook-library-client";

type Schema = components["schemas"];
export const libraryId = (index: number) =>
  `00000000-0000-0000-0000-${index.toString(16).padStart(12, "0")}`;

export async function prepareLibrary(
  page: Page,
  origin: string,
  remember: Remember,
  budget: LibraryBudget,
) {
  const suffix = process.env.HITO_LIBRARY_RUN_ID;
  if (!suffix || !/^[a-f0-9]{32}$/.test(suffix))
    throw new Error("The owned library run identity is missing");
  const prefix = `Private library ${suffix}`;
  const credentials = {
    username: `library-${suffix.slice(0, 12)}`,
    password: `owned library password ${suffix}`,
  };
  const peerCredentials = {
    username: `peer-${suffix.slice(0, 12)}`,
    password: `owned peer password ${suffix}`,
  };
  for (const value of [
    prefix,
    ...Object.values(credentials),
    ...Object.values(peerCredentials),
    "/music/",
    "/workspace/fixtures/e2e-generated/",
    "Fixture 440 Hz",
    "Fixture 660 Hz",
    "Hito test fixtures",
    "Fixture collection 440 Hz",
    "Fixture collection 660 Hz",
  ])
    remember(value);
  const fresh = await page.request.get("/api/v1/setup", { timeout: 10_000 });
  try {
    expect(fresh.status()).toBe(200);
    expect(await fresh.json()).toEqual({ needs_setup: true });
  } finally {
    await fresh.dispose();
  }
  const originals = await originalState();
  await page.goto("/");
  await page.getByLabel("Username", { exact: true }).fill(credentials.username);
  await page.getByLabel("Password", { exact: true }).fill(credentials.password);
  await page
    .getByRole("button", { name: "Create administrator", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "Your library", exact: true }),
  ).toBeVisible();
  for (const cookie of await page.context().cookies()) remember(cookie.value);
  const owner = await libraryClient(origin, credentials, budget, remember);
  let peer: LibraryClient | null = null;
  try {
    const settings = await owner.read<Schema["MetadataSettings"]>(
      "/api/v1/metadata/settings",
    );
    const disabled = await owner.call<Schema["MetadataSettings"]>(
      "PUT",
      "/api/v1/metadata/settings",
      200,
      { ...settings, enabled: false },
    );
    expect(disabled.enabled).toBe(false);
    const saved = await seedPreferencesLibrary(page, owner.read, owner.user);
    saved.privateValues.forEach(remember);
    await owner.call("POST", "/api/v1/users", 201, peerCredentials);
    peer = await libraryClient(origin, peerCredentials, budget, remember);
    const catalog =
      await owner.read<Schema["RecordingPage"]>("/api/v1/recordings");
    if (catalog.items.length !== 2)
      throw new Error("The owned library must contain exactly two originals");
    for (const library of new Set(
      catalog.items.map((recording) => recording.library_id),
    ))
      await owner.call(
        "PUT",
        `/api/v1/libraries/${library}/members/${peer.user.id}`,
        204,
      );
    const name = `${prefix} grouped`;
    remember(name);
    const books = Array.from({ length: 121 }, (_, index) => ({
      id: libraryId(index + 1),
      name,
      kind: "local" as const,
    }));
    const deleted = libraryId(122);
    const actions: Schema["Action"][] = books.flatMap((book) => [
      { kind: "playlist_create", playlist_id: book.id, name: book.name },
      { kind: "audiobook_create", playlist_id: book.id },
    ]);
    actions.push(
      {
        kind: "entry_insert",
        playlist_id: books[0].id,
        entry_id: randomUUID(),
        recording_id: saved.ids[0],
        position: 0,
      },
      { kind: "playlist_create", playlist_id: deleted, name },
      { kind: "audiobook_create", playlist_id: deleted },
      { kind: "playlist_delete", playlist_id: deleted },
    );
    await owner.sync(actions);
    const peerBook = randomUUID();
    const peerName = `${prefix} peer book`;
    const peerTitle = `${prefix} peer title`;
    remember(peerName);
    remember(peerTitle);
    await peer.sync([
      { kind: "playlist_create", playlist_id: peerBook, name: peerName },
      {
        kind: "entry_insert",
        playlist_id: peerBook,
        entry_id: randomUUID(),
        recording_id: saved.ids[0],
        position: 0,
      },
      { kind: "audiobook_create", playlist_id: peerBook },
      {
        kind: "recording_title_set",
        recording_id: saved.ids[0],
        title: peerTitle,
      },
      { kind: "like_set", recording_id: saved.ids[0], liked: true },
      {
        kind: "progress_set",
        recording_id: saved.ids[0],
        position_ms: 800,
        speed: 1.5,
      },
    ]);
    const result = {
      owner,
      peer,
      saved,
      books,
      deleted,
      peerBook,
      peerName,
      peerCredentials,
      originals,
      budget,
      remember,
      async snapshot() {
        return {
          owner: await librarySnapshot(
            owner,
            saved.ids,
            [saved.playlist, books[0].id],
            remember,
          ),
          peer: await librarySnapshot(peer!, saved.ids, [peerBook], remember),
        };
      },
      async absence() {
        const owned = [
          saved.playlist,
          books[0].id,
          books[59].id,
          books[120].id,
        ];
        return {
          owner: await downloadAbsence(owner, saved.ids, owned),
          peer: await downloadAbsence(peer!, saved.ids, [peerBook]),
        };
      },
      async originalsUnchanged() {
        return isDeepStrictEqual(await originalState(), originals);
      },
      async close() {
        await peer!.close();
        await owner.close();
      },
    };
    return result;
  } catch (error) {
    await peer?.close();
    await owner.close();
    throw error;
  }
}
export type LibraryFixture = Awaited<ReturnType<typeof prepareLibrary>>;

async function originalState() {
  const audio = await artistReviewOriginals();
  const metadata = await Promise.all(
    ["Music/Fixture 440 Hz.wav", "Archive/Fixture 660 Hz.wav"].map(
      async (name) => {
        const value = await stat(`/workspace/fixtures/e2e-generated/${name}`);
        return {
          size: value.size,
          mode: value.mode,
          uid: value.uid,
          gid: value.gid,
          ino: value.ino,
          mtime: value.mtimeMs,
        };
      },
    ),
  );
  return { audio, metadata };
}

async function librarySnapshot(
  client: LibraryClient,
  recordings: string[],
  playlists: string[],
  remember: Remember,
) {
  const progress = await client.call<Schema["RecordingProgress"][]>(
    "POST",
    "/api/v1/recordings/progress",
    200,
    { recording_ids: recordings },
  );
  return {
    user: await client.read<Schema["User"]>("/api/v1/me"),
    libraries: await client.read("/api/v1/libraries"),
    settings: await client.read("/api/v1/metadata/settings"),
    catalog: await client.read("/api/v1/recordings"),
    artists: await client.read("/api/v1/artists"),
    releases: await client.read("/api/v1/releases"),
    playlists: await client.read<Schema["Playlist"][]>("/api/v1/playlists"),
    books: await libraryBooks(client, remember),
    entries: await Promise.all(
      playlists.map((id) => client.read(`/api/v1/playlists/${id}`)),
    ),
    activity: await client.read("/api/v1/activity"),
    events: await events(client),
    progress,
    metadata: await Promise.all(
      recordings.map((id) => client.read(`/api/v1/recordings/${id}/metadata`)),
    ),
    sources: await Promise.all(
      recordings.map((id) => client.read(`/api/v1/recordings/${id}/sources`)),
    ),
  };
}

export async function libraryBooks(client: LibraryClient, remember: Remember) {
  const books: Schema["Playlist"][] = [];
  let cursor: string | null | undefined;
  for (let page = 0; page < 4; page++) {
    const result = await client.read<Schema["AudiobookLibraryPage"]>(
      `/api/v1/audiobooks/page?limit=60${cursor ? `&cursor=${encodeURIComponent(cursor)}` : ""}`,
    );
    if (result.items.length > 60 || result.pagination.limit !== 60)
      throw new Error("The owned book page exceeded its bound");
    books.push(...result.items);
    for (const value of [
      result.pagination.next_cursor,
      result.pagination.previous_cursor,
    ])
      if (value) remember(value);
    cursor = result.pagination.next_cursor;
    if (!cursor) return books;
  }
  throw new Error("The owned book traversal exceeded four pages");
}

async function events(client: LibraryClient) {
  const result: Schema["SyncEvent"][] = [];
  let cursor = 0;
  for (let page = 0; page < 3; page++) {
    const next = await client.read<Schema["SyncPage"]>(
      `/api/v1/sync?cursor=${cursor}`,
    );
    if (
      next.operations.length > 200 ||
      next.cursor < cursor ||
      (next.has_more && next.cursor === cursor)
    )
      throw new Error("The owned event page was invalid");
    result.push(...next.operations);
    cursor = next.cursor;
    if (!next.has_more) return result;
  }
  throw new Error("The owned event snapshot exceeded three pages");
}

async function downloadAbsence(
  client: LibraryClient,
  recordings: string[],
  playlists: string[],
) {
  const paths = [
    ...recordings.map((id) => `/api/v1/recordings/${id}/download`),
    ...playlists.map((id) => `/api/v1/playlists/${id}/download`),
  ];
  const result = [];
  for (const path of paths) {
    const value = await client.call<Schema["ErrorResponse"]>("GET", path, 404);
    if (!isDeepStrictEqual(value, { error: "Resource not found" }))
      throw new Error("The owned library acquired an implicit download");
    result.push({ status: 404, absent: true });
  }
  return result;
}
