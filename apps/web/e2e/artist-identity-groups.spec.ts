import { randomUUID } from "node:crypto";
import { isDeepStrictEqual } from "node:util";
import { expect, test, type Page } from "@playwright/test";
import type { Schema } from "../src/shared/schema";
import {
  artistReviewDownloadAbsence,
  artistReviewOriginals,
  artistReviewReader,
} from "./fixtures/artist-review-state";
import { artistIdentityGroupsProvider } from "./fixtures/artist-identity-groups-provider";
import {
  artistGroupsState,
  prepareArtistGroups,
} from "./fixtures/artist-identity-groups-state";
import {
  chooseGroupMember,
  resetGroupMember,
  type ChosenGroupMember,
} from "./fixtures/artist-identity-groups-journey";
import {
  reviewGroupMember,
  type ReviewedGroupMember,
} from "./fixtures/artist-identity-groups-review";
import { browseArtistGroups } from "./fixtures/artist-identity-groups-browsing";
import { artistGroupsEvidence } from "./fixtures/artist-identity-groups-evidence";
import { openLibrarySection } from "./fixtures/library-sections";

test.use({
  trace: "off",
  screenshot: "off",
  video: "off",
  launchOptions: { args: [] },
});
test.skip(
  process.env.HITO_ARTIST_IDENTITY_GROUPS_E2E !== "1",
  "Requires an opted-in fresh disposable artist group server",
);

test("reviewed choices group local and imported appearances, preserve namesakes and recover a lost acknowledgement", async ({
  page,
  context,
  baseURL,
}) => {
  test.setTimeout(180_000);
  page.setDefaultTimeout(10_000);
  page.setDefaultNavigationTimeout(15_000);
  let stage = "fresh disposable server";
  let failed = false;
  let armed = false;
  let checks: Record<string, boolean | number> = {};
  let provider: Awaited<
    ReturnType<typeof artistIdentityGroupsProvider>
  > | null = null;
  let evidence: Awaited<ReturnType<typeof artistGroupsEvidence>> | null = null;
  const password = `Owned private administrator password ${randomUUID()}`;
  const query = `Private artist group query ${randomUUID()}`;
  const privateValues = [
    password,
    query,
    "/music/",
    "Fixture 440 Hz",
    "Fixture 660 Hz",
    "Hito test fixtures",
    "Fixture collection 440 Hz",
    "Fixture collection 660 Hz",
  ];
  const read = artistReviewReader(page);
  try {
    expect(
      baseURL === "http://localhost:8080" &&
        (await read<Schema<"SetupStatus">>("/api/v1/setup")).needs_setup,
    ).toBe(true);
    evidence = await artistGroupsEvidence(context, baseURL!, privateValues);
    provider = await artistIdentityGroupsProvider(query);
    privateValues.push(...provider.privateValues, provider.origin);
    const originals = await artistReviewOriginals();

    stage = "explicit provider approval, folders and actual playlist import";
    const prepared = await prepareArtistGroups(page, read, provider, password);
    privateValues.push(
      ...prepared.saved.privateValues,
      ...(await context.cookies()).map((cookie) => cookie.value),
    );
    evidence.watch(prepared.user.id, prepared.saved.ids, prepared.playlist);
    expect(
      isDeepStrictEqual(provider.calls.slice(0, 3), [
        "manifest_public",
        "manifest_private",
        "manifest_private",
      ]) &&
        provider.calls.filter((call) => call === "playlist_detail").length ===
          2 &&
        provider.calls.filter((call) => call === "playlist_items").length === 1,
    ).toBe(true);

    stage = "search and preview namesakes, save reviews without choosing";
    const reviewed: ReviewedGroupMember[] = [];
    for (const member of prepared.members)
      reviewed.push(
        await reviewGroupMember(page, read, prepared, provider, member),
      );
    stage = "empty groups before choosing";
    const empty = await read<Schema<"ArtistIdentityGroupPage">>(
      "/api/v1/artist-identity-groups",
    );
    expect(empty.total === 0 && empty.items.length === 0).toBe(true);
    stage = "private collection baseline read";
    const before = await artistGroupsState(page, read, prepared);
    stage = "explicit download absence baseline";
    const downloads = await artistReviewDownloadAbsence(
      page,
      prepared.saved.ids,
    );
    stage = "private baseline predicates";
    const baselineChecks: Record<string, boolean> = {
      baselineLocalPlaylistCount: before.music.playlist.entries.length === 2,
      baselineLocalPlaylistEntryIds: isDeepStrictEqual(
        before.music.playlist.entries.map((entry) => entry.entry_id),
        prepared.saved.entries,
      ),
      baselineImportedPlaylistCount: before.imported.entries.length === 3,
      baselineImportedDuplicateRecording:
        before.imported.entries[0].recording_id ===
        before.imported.entries[2].recording_id,
      baselinePrivateTitle:
        before.music.metadata[0].title.personal ===
        prepared.saved.privateValues[0],
      baselinePrivateArtist:
        before.music.metadata[0].artist.personal ===
        prepared.saved.privateValues[1],
      baselineLikedRecording:
        before.music.catalog.items.find(
          (recording) => recording.id === prepared.saved.ids[0],
        )?.liked === true,
      baselineSavedProgress: before.music.progress.some(
        (item) =>
          item.recording_id === prepared.saved.ids[0] &&
          item.position_ms === 400 &&
          item.speed === 1.25,
      ),
      baselineOriginalSources: before.music.sources
        .slice(0, 2)
        .every((sources) => sources.length === 1),
      baselineImportedSources: before.music.sources
        .slice(2)
        .every((sources) => sources.length === 0),
    };
    checks = { ...checks, ...baselineChecks };
    expect(Object.values(baselineChecks).every(Boolean)).toBe(true);

    stage =
      "explicit revisioned choices and real accepted acknowledgement recovery";
    evidence.arm();
    armed = true;
    const chosen: ChosenGroupMember[] = [];
    for (const [index, review] of reviewed.entries())
      chosen.push(await chooseGroupMember(page, read, review, index === 0));
    const callsBeforeBrowse = [...provider.calls];

    stage =
      "chosen cards, original identities, independent pages, filters and reload";
    const groups = await browseArtistGroups(
      page,
      read,
      chosen,
      (phase, details) => {
        checks = { ...checks, ...details, browsingCheckpoint: phase };
      },
    );
    expect(isDeepStrictEqual(provider.calls, callsBeforeBrowse)).toBe(true);

    stage = "explicit reset removes links and retires empty groups";
    for (const [index, member] of chosen.entries()) {
      await resetGroupMember(page, read, member);
      if (index < 2) {
        const remaining = await read<Schema<"ArtistIdentityGroupDetail">>(
          `/api/v1/artist-identity-groups/${groups.groupId}?limit=200`,
        );
        expect(
          remaining.group.recording_count === 2 - index &&
            isDeepStrictEqual(
              new Set(
                remaining.recordings.items.map((item) => item.recording.id),
              ),
              new Set(
                chosen
                  .slice(index + 1, 3)
                  .map((item) => item.member.recording.id),
              ),
            ),
        ).toBe(true);
      } else
        await absentGroup(
          page,
          index === 2 ? groups.groupId : groups.namesakeId,
        );
    }
    await openLibrarySection(page, "Artists");
    await expect(
      page.getByRole("heading", { name: "No chosen artists yet", exact: true }),
    ).toBeVisible();
    await expect(
      page.getByRole("heading", { name: "Library credits", exact: true }),
    ).toBeVisible();
    const finalGroups = await read<Schema<"ArtistIdentityGroupPage">>(
      "/api/v1/artist-identity-groups",
    );
    expect(finalGroups.total === 0 && finalGroups.items.length === 0).toBe(
      true,
    );

    stage =
      "private collection, sources, duplicate entries and originals preserved";
    expect(
      isDeepStrictEqual(
        await artistGroupsState(page, read, prepared),
        before,
      ) &&
        isDeepStrictEqual(
          await artistReviewDownloadAbsence(page, prepared.saved.ids),
          downloads,
        ) &&
        isDeepStrictEqual(await artistReviewOriginals(), originals),
    ).toBe(true);
    expect(
      await page
        .locator("audio[data-hito-player]")
        .evaluate(
          (audio: HTMLAudioElement) =>
            audio.paused && audio.currentTime === 0 && !audio.currentSrc,
        ),
    ).toBe(true);
    expect(
      isDeepStrictEqual(provider.calls, callsBeforeBrowse) &&
        provider.calls.filter((call) => call === "search").length === 4,
    ).toBe(true);

    stage = "bounded requests and private-safe correlated workflow evidence";
    await expect.poll(() => evidence!.complete()).toBe(true);
    await evidence.finish();
    const counts = evidence.counts;
    expect(
      counts.foreign +
        counts.audio +
        counts.download +
        counts.sourceWrites +
        counts.unexpectedWrites +
        counts.invalidDownloadStatus +
        counts.pageErrors +
        counts.malformedLogs +
        counts.privateConsole +
        counts.bounds +
        counts.drainTimeout +
        provider.counters.unexpected +
        provider.counters.unsafeHeaders ===
        0 &&
        counts.downloadStatusReads > 0 &&
        counts.downloadStatusReads === counts.downloadStatusAbsentReplies,
    ).toBe(true);
  } catch {
    failed = true;
  } finally {
    try {
      await context.close();
    } catch {
      failed = true;
    }
    try {
      await provider?.close();
    } catch {
      failed = true;
    }
    try {
      await evidence?.finish(true);
    } catch {
      failed = true;
    }
    const counts = evidence?.counts;
    checks = {
      ...checks,
      ...(counts ?? {}),
      armed,
      workflowsComplete: evidence?.complete() ?? false,
      providerRequests: provider?.counters.requests ?? 0,
      unexpectedProviderRequests: provider?.counters.unexpected ?? 0,
      unsafeProviderHeaders: provider?.counters.unsafeHeaders ?? 0,
    };
    if (
      !evidence?.complete() ||
      !counts ||
      counts.foreign +
        counts.audio +
        counts.download +
        counts.sourceWrites +
        counts.unexpectedWrites +
        counts.invalidDownloadStatus +
        counts.pageErrors +
        counts.malformedLogs +
        counts.privateConsole +
        counts.bounds +
        counts.drainTimeout +
        (provider?.counters.unexpected ?? 0) +
        (provider?.counters.unsafeHeaders ?? 0) >
        0 ||
      counts.downloadStatusReads === 0 ||
      counts.downloadStatusReads !== counts.downloadStatusAbsentReplies
    )
      failed = true;
  }
  if (failed) {
    const summary = JSON.stringify(checks);
    if (Buffer.byteLength(summary) > 4096)
      throw new Error(
        "Artist group verification exceeded its safe diagnostic bound",
      );
    throw new Error(
      `Artist group verification failed at ${stage}; checks=${summary}`,
    );
  }
});

async function absentGroup(page: Page, group: string) {
  const response = await page.request.get(
    `/api/v1/artist-identity-groups/${group}`,
    { timeout: 10_000 },
  );
  try {
    const bytes = await response.body();
    if (bytes.length > 4096)
      throw new Error("Owned retired group response exceeded its bound");
    expect(
      response.status() === 404 &&
        isDeepStrictEqual(JSON.parse(bytes.toString()), {
          error: "Resource not found",
        } satisfies Schema<"ErrorResponse">),
    ).toBe(true);
  } finally {
    await response.dispose();
  }
}
