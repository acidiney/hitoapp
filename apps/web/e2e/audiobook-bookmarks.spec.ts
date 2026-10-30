import { randomUUID } from "node:crypto";
import { isDeepStrictEqual } from "node:util";
import { expect, test, type Page } from "@playwright/test";
import type { Schema } from "../src/shared/schema";
import { bookmarkDelivery } from "./fixtures/audiobook-bookmarks-delivery";
import { bookmarkEvidence } from "./fixtures/audiobook-bookmarks-evidence";
import {
  artistReviewDownloadAbsence,
  artistReviewOriginals,
  artistReviewReader,
} from "./fixtures/artist-review-state";
import { seedPreferencesLibrary } from "./fixtures/metadata-preferences-state";

type Saved = Awaited<ReturnType<typeof seedPreferencesLibrary>>;
test.use({
  trace: "off",
  screenshot: "off",
  video: "off",
  launchOptions: { args: [] },
});
test.skip(
  process.env.HITO_AUDIOBOOK_BOOKMARKS_E2E !== "1",
  "Requires an opted-in fresh disposable packaged audiobook bookmark server",
);

test("a private bookmark survives a lost accepted acknowledgement and reload before exact removal without changing audio", async ({
  page,
  context,
  baseURL,
}) => {
  test.setTimeout(120_000);
  page.setDefaultTimeout(10_000);
  page.setDefaultNavigationTimeout(15_000);
  let stage = "fresh disposable server";
  let failed = false;
  let evidence: Awaited<ReturnType<typeof bookmarkEvidence>> | null = null;
  let delivery: Awaited<ReturnType<typeof bookmarkDelivery>> | null = null;
  let apiRequests = 0;
  const password = `owned bookmark administrator password ${randomUUID()}`;
  const label = `Marcador privado: ação no segundo capítulo ${randomUUID()}`;
  const privateValues = [
    password,
    label,
    "/music/",
    "/workspace/fixtures/e2e-generated/",
    "Fixture 440 Hz",
    "Fixture 660 Hz",
    "Hito test fixtures",
    "Fixture collection 440 Hz",
    "Fixture collection 660 Hz",
  ];
  const checks = {
    created_once: false,
    replay_idempotent: false,
    reload_saved: false,
    removed_once: false,
    reload_empty: false,
    collection_preserved: false,
    originals_preserved: false,
    downloads_absent: false,
    audio_inactive: false,
    transport_clean: false,
    console_clean: false,
  };
  const diagnostics = { chapter_label_matches: 0, marker_rows: 0 };
  const rawRead = artistReviewReader(page);
  const budget = (amount = 1) => {
    apiRequests += amount;
    if (apiRequests > 100)
      throw new Error("The bookmark state request budget was exceeded");
  };
  const read = async <T>(path: string): Promise<T> => {
    budget();
    return rawRead<T>(path);
  };
  const rememberCookies = async () => {
    privateValues.push(
      ...(await context.cookies()).map((cookie) => cookie.value),
    );
  };
  const post = async <T>(
    path: string,
    account: string,
    data: Schema<"SyncBatch"> | Schema<"SavedPlaybackRequest">,
  ): Promise<T> => {
    budget();
    if (Buffer.byteLength(JSON.stringify(data)) > 16_384)
      throw new Error("The bookmark state request exceeded its bound");
    const response = await page.request.post(path, {
      headers: {
        "x-hito-request": "1",
        "x-hito-account": account,
        Origin: new URL(page.url()).origin,
      },
      data,
      timeout: 10_000,
    });
    try {
      const body = await response.body();
      if (response.status() !== 200 || body.length > 524_288)
        throw new Error("The bookmark state response exceeded its bound");
      return JSON.parse(body.toString()) as T;
    } finally {
      await response.dispose();
    }
  };
  try {
    expect(baseURL === "http://localhost:8080").toBe(true);
    const origin = new URL(baseURL!).origin;
    evidence = await bookmarkEvidence(context, origin, privateValues);
    delivery = await bookmarkDelivery(context, origin, evidence);
    expect(
      (await read<Schema<"SetupStatus">>("/api/v1/setup")).needs_setup,
    ).toBe(true);
    const originals = await artistReviewOriginals();
    await page.goto("/");
    await page.getByLabel("Username", { exact: true }).fill("bookmark-admin");
    await page.getByLabel("Password", { exact: true }).fill(password);
    await page
      .getByRole("button", { name: "Create administrator", exact: true })
      .click();
    await expect(
      page.getByRole("heading", { name: "Your library", exact: true }),
    ).toBeVisible();
    await rememberCookies();
    const user = await read<Schema<"User">>("/api/v1/me");

    stage = "synthetic folders and private preparation";
    const saved = await seedPreferencesLibrary(page, read, user);
    budget();
    privateValues.push(...saved.privateValues);
    const group = {
      device_id: "owned-bookmark-preparation",
      operations: [
        {
          id: randomUUID(),
          action: { kind: "audiobook_create", playlist_id: saved.playlist },
        },
      ],
    } satisfies Schema<"SyncBatch">;
    expect(
      isDeepStrictEqual(
        await post<Schema<"SyncReceipt">>("/api/v1/sync", user.id, group),
        { acknowledged: [group.operations[0].id] },
      ),
    ).toBe(true);
    const beforeBook = await read<Schema<"AudiobookDetail">>(
      `/api/v1/audiobooks/${saved.playlist}`,
    );
    expect(
      beforeBook.playlist.kind === "local" &&
        beforeBook.playlist.id === saved.playlist &&
        beforeBook.bookmarks.length === 0 &&
        beforeBook.chapters.length === 2 &&
        isDeepStrictEqual(
          beforeBook.chapters.map((chapter) => chapter.entry_id),
          saved.entries,
        ) &&
        beforeBook.chapters.every(
          (chapter) =>
            chapter.recording_id === saved.ids[0] && chapter.available,
        ),
    ).toBe(true);
    const before = await snapshot(page, read, post, budget, user.id, saved);
    expect(
      before.metadata[0].title.personal === saved.privateValues[0] &&
        before.metadata[0].artist.personal === saved.privateValues[1] &&
        before.catalog.items.find((recording) => recording.id === saved.ids[0])
          ?.liked === true &&
        before.progress.some(
          (item) =>
            item.recording_id === saved.ids[0] &&
            item.position_ms === 400 &&
            item.speed === 1.25,
        ) &&
        before.sources.every((sources) => sources.length === 1),
    ).toBe(true);
    const cursor = await read<Schema<"SyncPage">>("/api/v1/sync?cursor=0");
    expect(!cursor.has_more).toBe(true);
    budget(saved.ids.length);
    const absent = await artistReviewDownloadAbsence(page, saved.ids);
    await absentPlaylistDownload(page, saved.playlist, budget);
    await page.goto(`/audiobooks/${saved.playlist}`);
    await expect(
      page.getByRole("heading", {
        name: beforeBook.playlist.name,
        exact: true,
      }),
    ).toBeVisible();
    await expect(
      page
        .getByRole("list", { name: "Chapters", exact: true })
        .getByRole("listitem"),
    ).toHaveCount(2);
    await page.waitForLoadState("networkidle", { timeout: 10_000 });
    delivery.arm(user.id, beforeBook, beforeBook.chapters[1], label);
    evidence.arm();

    stage = "explicit second occurrence and lost accepted acknowledgement";
    await page
      .getByRole("combobox", { name: "Chapter", exact: true })
      .selectOption(saved.entries[1]);
    await page
      .getByRole("spinbutton", { name: "Position in seconds", exact: true })
      .fill("35.5");
    await page
      .getByRole("textbox", { name: "Bookmark label", exact: true })
      .fill(label);
    expect(delivery.counts.create + delivery.counts.remove).toBe(0);
    await page
      .getByRole("button", { name: "Save bookmark", exact: true })
      .click();
    await expect(
      page.getByRole("button", { name: "Retry bookmark change", exact: true }),
    ).toBeEnabled();
    expect(
      delivery.counts.create === 1 && evidence.counts.lost_failures === 1,
    ).toBe(true);
    const create = delivery.creation();
    const action = create.operations[0].action;
    if (action.kind !== "audiobook_bookmark_create")
      throw new Error("The bookmark create has no identity");
    privateValues.push(action.bookmark_id);
    const committed = await read<Schema<"AudiobookDetail">>(
      `/api/v1/audiobooks/${saved.playlist}`,
    );
    const createEvents = await read<Schema<"SyncPage">>(
      `/api/v1/sync?cursor=${cursor.cursor}`,
    );
    checks.created_once =
      committed.bookmarks.length === 1 &&
      committed.bookmarks[0].id === action.bookmark_id &&
      committed.bookmarks[0].entry_id === saved.entries[1] &&
      committed.bookmarks[0].recording_id === saved.ids[0] &&
      committed.bookmarks[0].position_ms === 35_500 &&
      committed.bookmarks[0].label === label &&
      !createEvents.has_more &&
      createEvents.operations.length === 1 &&
      "operation" in createEvents.operations[0] &&
      createEvents.operations[0].device_id === create.device_id &&
      isDeepStrictEqual(
        createEvents.operations[0].operation,
        create.operations[0],
      );
    expect(checks.created_once).toBe(true);
    const markers = page.getByRole("region", {
      name: "Bookmarks",
      exact: true,
    });
    await expect(markers.getByText(label, { exact: true })).toHaveCount(0);
    stage = "manual immutable retry and held owned confirmation";
    await page
      .getByRole("button", { name: "Retry bookmark change", exact: true })
      .click();
    await expect.poll(() => delivery!.counts.held_reads).toBe(1);
    await expect(markers.getByText(label, { exact: true })).toHaveCount(0);
    await expect(
      page.getByText("Bookmark saved.", { exact: true }),
    ).toHaveCount(0);
    delivery.release(1);
    await expect(markers.getByText(label, { exact: true })).toBeVisible();
    stage = "confirmed bookmark chapter label";
    diagnostics.chapter_label_matches = await markers
      .getByText(`Chapter 2: ${saved.privateValues[0]}`, { exact: true })
      .count();
    diagnostics.marker_rows = await markers.getByRole("listitem").count();
    await expect(
      markers
        .getByRole("listitem")
        .filter({ has: page.getByText(label, { exact: true }) })
        .getByText(`Chapter 2: ${saved.privateValues[0]}`, {
          exact: true,
        }),
    ).toBeVisible();
    checks.replay_idempotent =
      isDeepStrictEqual(
        await read(`/api/v1/audiobooks/${saved.playlist}`),
        committed,
      ) &&
      isDeepStrictEqual(
        await read(`/api/v1/sync?cursor=${cursor.cursor}`),
        createEvents,
      ) &&
      delivery.counts.replay === 1;
    expect(checks.replay_idempotent).toBe(true);

    stage = "saved bookmark survives a complete document reload";
    await page.reload({ waitUntil: "networkidle" });
    await rememberCookies();
    await expect(markers.getByText(label, { exact: true })).toBeVisible();
    checks.reload_saved = isDeepStrictEqual(
      await read(`/api/v1/audiobooks/${saved.playlist}`),
      committed,
    );
    expect(checks.reload_saved).toBe(true);
    stage = "explicit exact removal and held owned absence";
    const row = markers
      .getByRole("listitem")
      .filter({ has: page.getByText(label, { exact: true }) });
    await row
      .getByRole("button", { name: "Remove bookmark", exact: true })
      .click();
    await expect.poll(() => delivery!.counts.held_reads).toBe(2);
    await expect(markers.getByText(label, { exact: true })).toBeVisible();
    delivery.release(2);
    await expect(markers.getByRole("listitem")).toHaveCount(0);
    const removed = delivery.removal();
    const events = await read<Schema<"SyncPage">>(
      `/api/v1/sync?cursor=${cursor.cursor}`,
    );
    checks.removed_once =
      !events.has_more &&
      events.operations.length === 2 &&
      events.operations.every(
        (event, index) =>
          "operation" in event &&
          event.device_id === create.device_id &&
          isDeepStrictEqual(
            event.operation,
            [create.operations[0], removed.operations[0]][index],
          ),
      );
    expect(checks.removed_once).toBe(true);
    await page.reload({ waitUntil: "networkidle" });
    await rememberCookies();
    await expect(markers.getByRole("listitem")).toHaveCount(0);
    await expect(
      markers.getByText("No bookmarks yet.", { exact: true }),
    ).toBeVisible();
    checks.reload_empty = isDeepStrictEqual(
      await read(`/api/v1/audiobooks/${saved.playlist}`),
      beforeBook,
    );
    stage = "private collection originals and storage remain unchanged";
    checks.collection_preserved = isDeepStrictEqual(
      await snapshot(page, read, post, budget, user.id, saved),
      before,
    );
    checks.originals_preserved = isDeepStrictEqual(
      await artistReviewOriginals(),
      originals,
    );
    budget(saved.ids.length);
    checks.downloads_absent = isDeepStrictEqual(
      await artistReviewDownloadAbsence(page, saved.ids),
      absent,
    );
    await absentPlaylistDownload(page, saved.playlist, budget);
    checks.audio_inactive =
      (await page.locator("audio[data-hito-player]").count()) === 1 &&
      (await page.locator("audio").evaluateAll(
        (elements) =>
          elements.length === 1 &&
          elements.every((element) => {
            const audio = element as HTMLAudioElement;
            return (
              audio.paused &&
              audio.currentTime === 0 &&
              !audio.currentSrc &&
              !audio.getAttribute("src")
            );
          }),
      ));
    expect(Object.values(checks).slice(0, 9).every(Boolean)).toBe(true);
  } catch {
    failed = true;
  } finally {
    try {
      await rememberCookies();
    } catch {
      failed = true;
    }
    try {
      await delivery?.close();
    } catch {
      failed = true;
    }
    try {
      await context.close();
    } catch {
      failed = true;
    }
    try {
      await evidence?.finish();
    } catch {
      failed = true;
    }
  }
  if (evidence && delivery) {
    const counts = evidence.counts;
    checks.transport_clean =
      counts.foreign +
        counts.audio +
        counts.providers +
        counts.sources +
        counts.downloads +
        counts.unexpected_writes +
        counts.unexpected_failures +
        counts.invalid_responses +
        counts.page_errors +
        counts.invalid_phase +
        delivery.counts.invalid ===
        0 &&
      counts.lost_failures === 1 &&
      counts.workflow_requests <= 100 &&
      counts.preparation_requests + counts.workflow_requests <= 320 &&
      delivery.counts.create === 1 &&
      delivery.counts.replay === 1 &&
      delivery.counts.remove === 1 &&
      delivery.counts.held_reads === 2;
    checks.console_clean =
      counts.private_console + counts.malformed_console === 0 &&
      counts.console_bytes <= 524_288 &&
      counts.started === 3 &&
      counts.finished === 3 &&
      counts.confirmed === 2 &&
      counts.unconfirmed === 1;
  }
  if (!Object.values(checks).every(Boolean)) failed = true;
  const report = JSON.stringify({
    stage,
    failed,
    api_budget_used: apiRequests,
    checks,
    diagnostics,
    transport: evidence?.counts ?? {},
    delivery: delivery?.counts ?? {},
  });
  if (Buffer.byteLength(report) > 4096)
    throw new Error("The bookmark browser evidence exceeded its output bound");
  console.log(`audiobook_bookmarks_browser ${report}`);
  if (failed)
    throw new Error(
      `Audiobook bookmark browser verification failed at ${stage}`,
    );
});

async function snapshot(
  page: Page,
  read: ReturnType<typeof artistReviewReader>,
  post: <T>(
    path: string,
    account: string,
    data: Schema<"SyncBatch"> | Schema<"SavedPlaybackRequest">,
  ) => Promise<T>,
  budget: () => void,
  account: string,
  saved: Saved,
) {
  const choices = [];
  for (const id of saved.ids) {
    budget();
    const response = await page.request.get(
      `/api/v1/recordings/${id}/source-choice`,
      { timeout: 10_000 },
    );
    try {
      const body = await response.body();
      if (
        response.status() !== 404 ||
        body.length > 4096 ||
        !isDeepStrictEqual(JSON.parse(body.toString()), {
          error: "Resource not found",
        } satisfies Schema<"ErrorResponse">)
      )
        throw new Error("The original source choice changed");
      choices.push({ status: 404, choice: null });
    } finally {
      await response.dispose();
    }
  }
  return {
    libraries: await read<Schema<"Library">[]>("/api/v1/libraries"),
    catalog: await read<Schema<"RecordingPage">>("/api/v1/recordings"),
    artists: await read<Schema<"ArtistPage">>("/api/v1/artists"),
    releases: await read<Schema<"ReleasePage">>("/api/v1/releases"),
    playlist: await read<Schema<"PlaylistDetail">>(
      `/api/v1/playlists/${saved.playlist}`,
    ),
    activity: await read<Schema<"Activity">>("/api/v1/activity"),
    progress: await post<Schema<"RecordingProgress">[]>(
      "/api/v1/recordings/progress",
      account,
      { recording_ids: saved.ids } satisfies Schema<"SavedPlaybackRequest">,
    ),
    metadata: await Promise.all(
      saved.ids.map((id) =>
        read<Schema<"RecordingMetadata">>(`/api/v1/recordings/${id}/metadata`),
      ),
    ),
    sources: await Promise.all(
      saved.ids.map((id) =>
        read<Schema<"Source">[]>(`/api/v1/recordings/${id}/sources`),
      ),
    ),
    choices,
  };
}

async function absentPlaylistDownload(
  page: Page,
  playlist: string,
  budget: () => void,
) {
  budget();
  const response = await page.request.get(
    `/api/v1/playlists/${playlist}/download`,
    { timeout: 10_000 },
  );
  try {
    const body = await response.body();
    if (
      response.status() !== 404 ||
      body.length > 4096 ||
      !isDeepStrictEqual(JSON.parse(body.toString()), {
        error: "Resource not found",
      } satisfies Schema<"ErrorResponse">)
    )
      throw new Error(
        "The playlist acquired an unrequested persistent download",
      );
  } finally {
    await response.dispose();
  }
}
