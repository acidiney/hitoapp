import { isDeepStrictEqual } from "node:util";
import { expect, test, type Page } from "@playwright/test";
import type { components } from "../src/generated";
import {
  artistReviewDownloadAbsence,
  artistReviewOriginals,
  artistReviewReader,
} from "./fixtures/artist-review-state";
import {
  preferencesLibraryState,
  seedPreferencesLibrary,
} from "./fixtures/metadata-preferences-state";
import {
  audiobookDownloadAbsence,
  audiobookGroupingDelivery,
  failureScope,
} from "./fixtures/audiobook-grouping-delivery";
import { audiobookGroupingEvidence } from "./fixtures/audiobook-grouping-evidence";
import { openLibrarySection } from "./fixtures/library-sections";

type Schema = components["schemas"];
test.use({
  trace: "off",
  screenshot: "off",
  video: "off",
  launchOptions: { args: [] },
});
test.skip(
  process.env.HITO_AUDIOBOOK_GROUPING_E2E !== "1",
  "Requires an opted-in fresh disposable audiobook grouping server",
);

test("request start owns the failure boundary and only the exact lost delivery is exempt", () => {
  const lost = { path: "sync", account: "owned", failurePhase: "grouping" };
  expect(failureScope(lost, lost, true)).toBe("lost");
  for (const request of [
    { ...lost },
    { ...lost, path: "source" },
    { ...lost, account: "other" },
    { ...lost, account: "absent" },
    { ...lost, failurePhase: "fresh disposable server" },
  ]) {
    expect(failureScope(request, lost, true)).toBe("unexpected");
    expect(failureScope(request, lost, undefined)).toBe("unexpected");
  }
  expect(failureScope(lost, null, true)).toBe("unexpected");
  expect(failureScope(lost, lost, false)).toBe("unexpected");
  expect(failureScope(lost, lost, undefined)).toBe("unexpected");
  for (const path of ["setup", "account_self", "owned_folder_list", "other"]) {
    const request = { ...lost, path, failurePhase: "grouping" };
    expect(failureScope(request, lost, false)).toBe("preparation");
    expect(failureScope(request, lost, true)).toBe("unexpected");
    expect(failureScope(request, lost, undefined)).toBe("unexpected");
  }
});

test("late registered private canaries are checked against previously captured console messages", async ({
  page,
  context,
  baseURL,
}) => {
  test.setTimeout(15_000);
  const privateValues: string[] = [];
  const canary = "owned late registered console session canary";
  let evidence: Awaited<ReturnType<typeof audiobookGroupingEvidence>> | null =
    null;
  let privateConsoleDetected = false;
  let failed = false;
  try {
    if (!baseURL) throw new Error("The owned browser origin is missing");
    evidence = await audiobookGroupingEvidence(
      context,
      baseURL,
      privateValues,
      () => "late registered console canary check",
    );
    await page.goto("about:blank");
    await page.evaluate((value) => console.info(value), canary);
    await expect.poll(() => evidence!.counts.snapshots).toBeGreaterThan(0);
    await evidence.finish();
    privateValues.push(canary);
    await evidence.finish();
    privateConsoleDetected = evidence.counts.privateConsole > 0;
  } catch {
    failed = true;
  } finally {
    try {
      await context.close();
    } catch {
      failed = true;
    }
    await evidence?.finish();
  }
  if (failed)
    throw new Error("The owned late-canary console capture did not complete");
  expect(privateConsoleDetected).toBe(true);
});

test("a lost grouping acknowledgement retries one exact operation and retains private duplicate chapters", async ({
  page,
  context,
  baseURL,
  browserName,
}) => {
  test.setTimeout(120_000);
  page.setDefaultTimeout(10_000);
  page.setDefaultNavigationTimeout(15_000);
  let stage = "fresh disposable server";
  let checks: Record<string, boolean | number> = {};
  let failed = false;
  let groupingEvents = 0;
  let chapterOccurrences = 0;
  let privateStateRetained = false;
  let originalsAndDownloadAbsenceVerified = false;
  let audioInactiveVerified = false;
  let delivery: Awaited<ReturnType<typeof audiobookGroupingDelivery>> | null =
    null;
  let evidence: Awaited<ReturnType<typeof audiobookGroupingEvidence>> | null =
    null;
  const password = "owned audiobook browser administrator password";
  const privateValues = [
    password,
    "/music/",
    "/workspace/fixtures/e2e-generated/",
    "Fixture 440 Hz.wav",
    "Fixture 660 Hz.wav",
    "Fixture 440 Hz",
    "Fixture 660 Hz",
    "Hito test fixtures",
    "Fixture collection 440 Hz",
    "Fixture collection 660 Hz",
  ];
  const read = artistReviewReader(page);
  try {
    expect(baseURL === "http://localhost:8080").toBe(true);
    expect(
      (await read<Schema["SetupStatus"]>("/api/v1/setup")).needs_setup,
    ).toBe(true);
    evidence = await audiobookGroupingEvidence(
      context,
      baseURL!,
      privateValues,
      () => stage,
    );
    delivery = await audiobookGroupingDelivery(context, baseURL!, () => stage);
    const originalsBefore = await artistReviewOriginals();
    await page.goto("/");
    await page.getByLabel("Username", { exact: true }).fill("admin");
    await page.getByLabel("Password", { exact: true }).fill(password);
    await page
      .getByRole("button", { name: "Create administrator", exact: true })
      .click();
    await expect(
      page.getByRole("heading", { name: "Your library", exact: true }),
    ).toBeVisible();
    privateValues.push(
      ...(await context.cookies()).map((cookie) => cookie.value),
    );
    const user = await read<Schema["User"]>("/api/v1/me");

    stage = "owned originals and private duplicate playlist";
    const saved = await seedPreferencesLibrary(page, read, user);
    privateValues.push(
      ...saved.privateValues,
      saved.playlist,
      ...saved.entries,
    );
    const before = await preferencesLibraryState(page, read, saved);
    const downloads = await artistReviewDownloadAbsence(page, saved.ids);
    const bookDownload = await audiobookDownloadAbsence(page, saved.playlist);
    checks = {
      recordings: before.catalog.total,
      chapters: before.playlist.entries.length,
      titlePinned: before.metadata[0].title.personal === saved.privateValues[0],
      artistPinned:
        before.metadata[0].artist.personal === saved.privateValues[1],
    };
    expect(before.events.has_more).toBe(false);
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
        before.catalog.items.find((recording) => recording.id === saved.ids[0])
          ?.liked === true &&
        before.progress.some(
          (progress) =>
            progress.recording_id === saved.ids[0] &&
            progress.position_ms === 400 &&
            progress.speed === 1.25,
        ) &&
        before.sources.every((sources) => sources.length === 1),
    ).toBe(true);
    expect(
      (await read<Schema["Playlist"][]>("/api/v1/audiobooks")).length,
    ).toBe(0);
    const expectedDetail = {
      playlist: before.playlist.playlist,
      chapters: before.playlist.entries,
      bookmarks: [],
    } satisfies Schema["AudiobookDetail"];
    delivery.arm(
      user.id,
      expectedDetail,
      saved.ids,
      before.libraries.map((library) => library.id),
    );

    stage = "sidebar audiobook entry and explicit playlist selection";
    await openLibrarySection(page, "Audiobooks");
    await expect(
      page
        .locator("main")
        .getByRole("heading", { level: 1, name: "Audiobooks", exact: true }),
    ).toBeVisible();
    const form = page.getByRole("region", {
      name: "Add an audiobook",
      exact: true,
    });
    const selection = form.getByRole("combobox", {
      name: "Playlist",
      exact: true,
    });
    await expect(
      selection.getByRole("option", {
        name: saved.privateValues[2],
        exact: true,
      }),
    ).toHaveCount(1);
    expect(delivery.counts.syncPosts).toBe(0);
    await selection.selectOption(saved.playlist);
    expect(delivery.counts.syncPosts).toBe(0);

    stage = "real accepted sync reply lost before browser delivery";
    await form
      .getByRole("button", { name: "Add audiobook", exact: true })
      .click();
    await expect(form.getByRole("alert")).toBeVisible();
    await expect(
      form.getByRole("button", { name: "Retry adding audiobook", exact: true }),
    ).toBeEnabled();
    await expect.poll(() => delivery!.counts.lostFailures).toBe(1);
    checks = { ...delivery.counts, deliveryFailed: delivery.state.failed };
    expect(
      delivery.counts.forwarded === 1 &&
        delivery.counts.acceptedOriginal === 1 &&
        delivery.counts.syncPosts === 1 &&
        delivery.counts.detailReads === 0 &&
        delivery.counts.bookVisits === 0 &&
        delivery.state.bodies.length === 1,
    ).toBe(true);
    const original = delivery.state.batch;
    if (!original || !delivery.state.receipt)
      throw new Error("The owned original sync receipt is missing");
    const committed = await read<Schema["SyncPage"]>("/api/v1/sync?cursor=0");
    expect(singleGroupingEvent(before.events, committed, original)).toBe(true);
    expect(
      isDeepStrictEqual(
        await read(`/api/v1/audiobooks/${saved.playlist}`),
        expectedDetail,
      ),
    ).toBe(true);

    stage = "exact replay authoritative read and private book navigation";
    await form
      .getByRole("button", { name: "Retry adding audiobook", exact: true })
      .click();
    await assertChapters(page, saved);
    await delivery.finish();
    checks = {
      ...delivery.counts,
      deliveryFailed: delivery.state.failed,
      sameBodies:
        delivery.state.bodies.length === 2 &&
        delivery.state.bodies[0] === delivery.state.bodies[1],
    };
    expect(
      delivery.state.bodies.length === 2 &&
        delivery.state.bodies[0] === delivery.state.bodies[1] &&
        delivery.counts.acceptedReplay === 1 &&
        delivery.counts.detailReads === 1 &&
        delivery.counts.bookVisits === 1 &&
        !delivery.state.failed,
    ).toBe(true);
    expect(
      isDeepStrictEqual(delivery.state.order, [
        "original_accepted",
        "retry_request",
        "retry_response",
        "detail_request",
        "detail_response",
        "book_visit",
      ]),
    ).toBe(true);
    expect(
      isDeepStrictEqual(
        await read<Schema["Playlist"][]>("/api/v1/audiobooks"),
        [before.playlist.playlist],
      ),
    ).toBe(true);

    stage = "reload list and book revisit retain occurrence identities";
    await page.reload();
    await assertChapters(page, saved);
    await page
      .locator("main")
      .getByRole("link", { name: "All audiobooks", exact: true })
      .click();
    await expect(
      page
        .locator("main")
        .getByRole("heading", { level: 1, name: "Audiobooks", exact: true }),
    ).toBeVisible();
    await expect(
      page
        .getByRole("region", { name: "Add an audiobook", exact: true })
        .getByRole("option", { name: saved.privateValues[2], exact: true }),
    ).toHaveCount(0);
    await page
      .locator("main")
      .getByRole("link", { name: saved.privateValues[2], exact: true })
      .click();
    await assertChapters(page, saved);

    stage =
      "private library snapshots exact single event and originals retained";
    const after = await preferencesLibraryState(page, read, saved);
    const expected = { ...before, events: after.events };
    checks = Object.fromEntries(
      (Object.keys(before) as (keyof typeof before)[])
        .filter((key) => key !== "events")
        .map((key) => [key, isDeepStrictEqual(before[key], after[key])]),
    );
    expect(
      isDeepStrictEqual(after, expected) &&
        isDeepStrictEqual(after.events, committed) &&
        singleGroupingEvent(before.events, after.events, original),
    ).toBe(true);
    groupingEvents =
      after.events.operations.length - before.events.operations.length;
    chapterOccurrences = after.playlist.entries.length;
    privateStateRetained = isDeepStrictEqual(after, expected);
    expect(
      isDeepStrictEqual(await artistReviewOriginals(), originalsBefore) &&
        isDeepStrictEqual(
          await artistReviewDownloadAbsence(page, saved.ids),
          downloads,
        ) &&
        isDeepStrictEqual(
          await audiobookDownloadAbsence(page, saved.playlist),
          bookDownload,
        ),
    ).toBe(true);
    originalsAndDownloadAbsenceVerified = true;
    expect(
      await page
        .locator("audio[data-hito-player]")
        .evaluate(
          (audio: HTMLAudioElement) =>
            audio.paused && audio.currentTime === 0 && !audio.currentSrc,
        ),
    ).toBe(true);
    audioInactiveVerified = true;

    stage = "bounded private browser and workflow evidence";
    await delivery.finish();
    await expect.poll(() => evidence!.counts.confirmed).toBe(1);
    await evidence.finish();
    const traffic = delivery.counts;
    const logs = evidence.counts;
    checks = { ...traffic, ...logs, deliveryFailed: delivery.state.failed };
    expect(
      !delivery.state.failed &&
        traffic.requests <= 250 &&
        traffic.syncPosts === 2 &&
        traffic.forwarded === 1 &&
        traffic.acceptedOriginal === 1 &&
        traffic.acceptedReplay === 1 &&
        traffic.detailReads === 1 &&
        traffic.bookVisits === 3 &&
        traffic.lostFailures === 1 &&
        traffic.preparationApiFailures <= traffic.requests &&
        traffic.rawApiFailures ===
          traffic.preparationApiFailures + traffic.unexpectedFailures &&
        traffic.foreign +
          traffic.audio +
          traffic.download +
          traffic.catalog +
          traffic.sourceWrites +
          traffic.unexpectedWrites +
          traffic.unexpectedOtherFailures +
          traffic.unexpectedFailures ===
          0 &&
        traffic.downloadStatusReads === traffic.absentStatuses &&
        traffic.downloadStatusReads <= 8 &&
        logs.snapshots > 0 &&
        logs.consoleBytes <= 524_288 &&
        logs.groupStarted === 2 &&
        logs.unconfirmed === 1 &&
        logs.confirmed === 1 &&
        logs.navigationStarted === 1 &&
        logs.navigationFinished === 1 &&
        logs.privateConsole + logs.malformedLogs + logs.pageErrors === 0,
    ).toBe(true);
  } catch {
    failed = true;
  } finally {
    try {
      await context.close();
    } catch {
      failed = true;
    }
    await delivery?.finish();
    await evidence?.finish();
    if (
      delivery?.state.failed ||
      (evidence &&
        evidence.counts.privateConsole +
          evidence.counts.malformedLogs +
          evidence.counts.pageErrors >
          0)
    )
      failed = true;
  }
  if (failed)
    throw new Error(
      `Audiobook grouping browser verification failed at ${stage}; checks=${JSON.stringify(checks)}; page_errors=${JSON.stringify(evidence?.errors ?? [])}; request_failures=${JSON.stringify(delivery?.failureDetails() ?? [])}`,
    );
  if (!delivery || !evidence)
    throw new Error("Audiobook grouping browser evidence is missing");
  const summary = JSON.stringify({
    schema: "hito.audiobook-grouping.browser",
    version: 1,
    outcome: "passed",
    engine: browserName,
    preparationScope: "observed_not_transport_qualified",
    failureScope: "requests_started_after_arm",
    requests: delivery.counts.requests,
    rawApiFailures: delivery.counts.rawApiFailures,
    preparationApiFailures: delivery.counts.preparationApiFailures,
    unexpectedFailures: delivery.counts.unexpectedFailures,
    unexpectedOtherFailures: delivery.counts.unexpectedOtherFailures,
    lostFailures: delivery.counts.lostFailures,
    foreign: delivery.counts.foreign,
    audio: delivery.counts.audio,
    download: delivery.counts.download,
    catalog: delivery.counts.catalog,
    sourceWrites: delivery.counts.sourceWrites,
    unexpectedWrites: delivery.counts.unexpectedWrites,
    syncPosts: delivery.counts.syncPosts,
    forwarded: delivery.counts.forwarded,
    acceptedOriginal: delivery.counts.acceptedOriginal,
    acceptedReplay: delivery.counts.acceptedReplay,
    detailReads: delivery.counts.detailReads,
    bookVisits: delivery.counts.bookVisits,
    downloadStatusReads: delivery.counts.downloadStatusReads,
    absentStatuses: delivery.counts.absentStatuses,
    deliveryFailed: delivery.state.failed,
    identicalRetryBodies:
      delivery.state.bodies.length === 2 &&
      delivery.state.bodies[0] === delivery.state.bodies[1],
    groupingEvents,
    chapterOccurrences,
    privateStateRetained,
    originalsAndDownloadAbsenceVerified,
    audioInactiveVerified,
    consoleBytes: evidence.counts.consoleBytes,
    snapshots: evidence.counts.snapshots,
    privateConsole: evidence.counts.privateConsole,
    malformedLogs: evidence.counts.malformedLogs,
    pageErrors: evidence.counts.pageErrors,
    groupStarted: evidence.counts.groupStarted,
    unconfirmed: evidence.counts.unconfirmed,
    confirmed: evidence.counts.confirmed,
    navigationStarted: evidence.counts.navigationStarted,
    navigationFinished: evidence.counts.navigationFinished,
  });
  if (Buffer.byteLength(summary) > 4096)
    throw new Error("Audiobook grouping browser evidence exceeded its bound");
  console.info(summary);
});

function singleGroupingEvent(
  before: Schema["SyncPage"],
  after: Schema["SyncPage"],
  batch: Schema["SyncBatch"],
) {
  const events = after.operations;
  const appended = events[before.operations.length];
  return (
    !before.has_more &&
    !after.has_more &&
    events.length === before.operations.length + 1 &&
    isDeepStrictEqual(
      events.slice(0, before.operations.length),
      before.operations,
    ) &&
    appended !== undefined &&
    "operation" in appended &&
    appended.device_id === batch.device_id &&
    isDeepStrictEqual(appended.operation, batch.operations[0]) &&
    appended.operation.action.kind === "audiobook_create"
  );
}

async function assertChapters(
  page: Page,
  saved: Awaited<ReturnType<typeof seedPreferencesLibrary>>,
) {
  const main = page.locator("main");
  await expect(
    main.getByRole("heading", {
      level: 1,
      name: saved.privateValues[2],
      exact: true,
    }),
  ).toBeVisible();
  const rows = main
    .getByRole("table", { name: "Chapters", exact: true })
    .locator("tbody tr");
  await expect(rows).toHaveCount(2);
  expect(
    isDeepStrictEqual(
      await rows.evaluateAll((items) => items.map((item) => item.id)),
      saved.entries.map((entry) => `chapter-${entry}`),
    ),
  ).toBe(true);
  for (let index = 0; index < 2; index++) {
    await expect(
      rows
        .nth(index)
        .getByRole("rowheader", { name: `Chapter ${index + 1}`, exact: true }),
    ).toBeVisible();
    await expect(
      rows
        .nth(index)
        .getByRole("cell", { name: saved.privateValues[0], exact: true }),
    ).toBeVisible();
  }
  expect(
    (await main
      .getByRole("link", { name: "Open playlist", exact: true })
      .getAttribute("href")) === `/playlists/${saved.playlist}`,
  ).toBe(true);
}
