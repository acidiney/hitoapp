import { randomUUID } from "node:crypto";
import { isDeepStrictEqual } from "node:util";
import { expect, test, type Page } from "@playwright/test";
import type { Schema } from "../src/shared/schema";
import { reviewPluginInstallation } from "./fixtures/plugin-installation-review";
import { artistDiscographyEvidence } from "./fixtures/artist-discography-evidence";
import { browseArtistEditions } from "./fixtures/artist-editions-browsing";
import { artistReviewProvider } from "./fixtures/artist-review-provider";
import {
  artistReviewDownloadAbsence,
  artistReviewOriginals,
  artistReviewReader,
} from "./fixtures/artist-review-state";
import {
  addMetadataProvider,
  openPreferences,
  removeMetadataProvider,
  preferencesAction,
  preferencesLibraryState,
  savePreferences,
  seedPreferencesLibrary,
} from "./fixtures/metadata-preferences-state";
import { openMetadataEditors } from "./fixtures/track-panels";

const editionsEnabled = process.env.HITO_ARTIST_EDITIONS_E2E === "1";
const expectedProviderRequests = editionsEnabled ? 23 : 17;
test.use({
  trace: "off",
  screenshot: "off",
  video: "off",
  launchOptions: { args: [] },
});
test.skip(
  process.env.HITO_ARTIST_DISCOGRAPHY_E2E !== "1" ||
    process.env.HITO_ARTIST_REVIEW_E2E !== "1",
  "Requires an opted-in fresh disposable discography server",
);

test(
  editionsEnabled
    ? "a reviewed namesake explicitly browses editions and selects a complete bonus track list without adopting or playing music"
    : "a reviewed namesake browses catalog releases explicitly without adopting or playing music",
  async ({ page, context, baseURL }) => {
    test.setTimeout(120_000);
    page.setDefaultTimeout(10_000);
    page.setDefaultNavigationTimeout(15_000);
    let stage = "fresh disposable server";
    let failed = false;
    let armed = false;
    let failedBeforeArm = false;
    let owned: Awaited<ReturnType<typeof artistReviewProvider>> | null = null;
    let evidence: Awaited<ReturnType<typeof artistDiscographyEvidence>> | null =
      null;
    let checks: Record<string, boolean | number> = {};
    let publicReads = 0;
    const boundedRead = artistReviewReader(page);
    const read = async <T>(path: string) => {
      publicReads++;
      return boundedRead<T>(path);
    };
    const password = "owned discography browser administrator password";
    const query = `Private discography namesake query ${randomUUID()}`;
    const draft = `Private unsaved discography title ${randomUUID()}`;
    const privateValues = [
      password,
      query,
      draft,
      "/music/",
      "Fixture 440 Hz",
      "Fixture 660 Hz",
      "Hito test fixtures",
      "Fixture collection 440 Hz",
      "Fixture collection 660 Hz",
    ];
    try {
      expect(baseURL === "http://localhost:8080").toBe(true);
      expect(
        (await read<Schema<"SetupStatus">>("/api/v1/setup")).needs_setup,
      ).toBe(true);
      evidence = await artistDiscographyEvidence(
        context,
        baseURL!,
        privateValues,
        editionsEnabled,
      );
      owned = await artistReviewProvider(query, {
        discography: true,
        editions: editionsEnabled,
      });
      const provider = owned;
      const catalog = provider.discography;
      if (!catalog) throw new Error("The opted-in catalog fixture is missing");
      const editions = provider.editions;
      if (editionsEnabled && !editions)
        throw new Error("The opted-in edition fixture is missing");
      if (editions) privateValues.push(...editions.privateValues);
      privateValues.push(
        provider.token,
        provider.origin,
        provider.manifest.name,
        provider.manifest.id,
        provider.profile.revision,
        provider.profile.disambiguation,
        catalog.cursor,
        catalog.title,
        catalog.credit,
        ...provider.candidates.flatMap((candidate) => [
          candidate.artist.id,
          candidate.artist.name,
          candidate.disambiguation,
        ]),
        ...[...catalog.first.items, ...catalog.last.items].map(
          (group) => group.id,
        ),
        ...catalog.first.items.flatMap((group) =>
          group.artist_credit.map((member) => member.credited_name),
        ),
      );
      const originals = await artistReviewOriginals();
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
      const user = await read<Schema<"User">>("/api/v1/me");

      stage = "owned originals and private duplicate playlist preparation";
      const saved = await seedPreferencesLibrary(page, read, user);
      privateValues.push(...saved.privateValues);
      const middle = await insertMiddleRecording(page, saved, user);
      evidence.watchDownloadStatus(user.id, saved.ids[0]);
      const prepared = await preferencesLibraryState(page, read, saved);
      expect(
        isDeepStrictEqual(
          prepared.playlist.entries.map((entry) => entry.recording_id),
          [saved.ids[0], saved.ids[1], saved.ids[0]],
        ) &&
          isDeepStrictEqual(
            prepared.playlist.entries.map((entry) => entry.entry_id),
            [saved.entries[0], middle, saved.entries[1]],
          ) &&
          prepared.metadata[0].title.personal === saved.privateValues[0] &&
          prepared.metadata[0].artist.personal === saved.privateValues[1] &&
          prepared.catalog.items.find((item) => item.id === saved.ids[0])
            ?.liked === true &&
          prepared.progress.some(
            (item) =>
              item.recording_id === saved.ids[0] &&
              item.position_ms === 400 &&
              item.speed === 1.25,
          ) &&
          prepared.sources.every((sources) => sources.length === 1),
      ).toBe(true);

      stage = "personal provider approval through settings";
      await page
        .getByRole("navigation", { name: "Settings sections" })
        .getByRole("link", { name: "Plugins", exact: true })
        .click();
      await page.getByRole("button", { name: /^This server/ }).click();
      await page
        .getByLabel("Service address", { exact: true })
        .fill(provider.address);
      const offered = await preferencesAction<Schema<"PluginService">>(
        page,
        page.getByRole("button", { name: "Offer service", exact: true }),
        "POST",
        "/api/v1/plugins/services",
        201,
      );
      if (!offered.body) throw new Error("The owned service offer is missing");
      expect(
        isDeepStrictEqual(offered.sent, { base_url: provider.address }) &&
          offered.body.name === provider.manifest.name,
      ).toBe(true);
      await page
        .getByRole("button", { name: "Your plugins", exact: true })
        .click();
      await page
        .getByRole("button", {
          name: `Connect ${provider.manifest.name}`,
          exact: true,
        })
        .click();
      await page
        .getByLabel("Bearer token (optional)", { exact: true })
        .fill(provider.token);
      const reviewed = await reviewPluginInstallation(
        page,
        offered.body,
        provider.manifest.name,
      );
      const installed = await preferencesAction<Schema<"PluginInstallation">>(
        page,
        page.getByRole("button", { name: "Accept and connect", exact: true }),
        "POST",
        "/api/v1/plugins",
        201,
      );
      if (!installed.body)
        throw new Error("The owned catalog approval is missing");
      expect(
        installed.body.credential_set &&
          installed.body.service_id === offered.body.id &&
          isDeepStrictEqual(installed.body.manifest, provider.manifest) &&
          isDeepStrictEqual(installed.sent, {
            ...reviewed.sent,
            accept_manifest_digest: reviewed.body.digest,
          }),
      ).toBe(true);
      const selected = {
        kind: "http_plugin",
        installation_id: installed.body.id,
      } satisfies Schema<"MetadataProvider">;
      const panel = await openPreferences(page);
      await removeMetadataProvider(panel, "MusicBrainz");
      await addMetadataProvider(panel, provider.manifest.name);
      await savePreferences(page, {
        revision: 0,
        enabled: true,
        providers: [selected],
      });
      expect(
        isDeepStrictEqual(provider.calls, [
          "manifest_public",
          "manifest_private",
          "manifest_private",
        ]),
      ).toBe(true);

      stage = "explicit second namesake search preview and confirmation";
      await page.goto(`/recordings/${saved.ids[0]}`);
      await openMetadataEditors(page);
      await page.getByLabel("Track title", { exact: true }).fill(draft);
      const artist = page.getByRole("region", {
        name: "Linked artist",
        exact: true,
      });
      const associationsPath = `/api/v1/recordings/${saved.ids[0]}/artist-associations`;
      const review = `/api/v1/recordings/${saved.ids[0]}/artist-review`;
      await artist
        .getByRole("button", { name: "Find artist information", exact: true })
        .click();
      await expect(
        artist.getByRole("combobox", {
          name: "Metadata provider",
          exact: true,
        }),
      ).toHaveValue(installed.body.id);
      await artist
        .getByRole("searchbox", { name: "Search artists", exact: true })
        .fill(query);
      const found = await preferencesAction<
        Schema<"ArtistCandidateSearchAvailability">
      >(
        page,
        artist.getByRole("button", { name: "Search artists", exact: true }),
        "POST",
        `${review}/search`,
        200,
      );
      if (found.body?.status !== "available")
        throw new Error("Owned artist candidates were unavailable");
      const captured = found.body.context;
      expect(
        captured.account_id === user.id &&
          captured.recording_id === saved.ids[0] &&
          captured.credited_name === saved.privateValues[1] &&
          isDeepStrictEqual(captured.provider, selected) &&
          isDeepStrictEqual(found.body.page.items, provider.candidates) &&
          isDeepStrictEqual(found.sent, {
            context: captured,
            search: provider.search,
          }),
      ).toBe(true);
      const results = artist.getByRole("list", {
        name: "Artist search results",
        exact: true,
      });
      await expect(results.getByRole("listitem")).toHaveCount(2);
      const second = results
        .getByRole("listitem")
        .filter({ hasText: provider.candidates[1].disambiguation });
      const selection = {
        context: captured,
        provider_artist_id: provider.profile.artist.id,
      } satisfies Schema<"SelectArtistCandidate">;
      const previewed = await preferencesAction<
        Schema<"ArtistCandidatePreviewAvailability">
      >(
        page,
        second.getByRole("button", { name: "Review artist", exact: true }),
        "POST",
        `${review}/preview`,
        200,
      );
      expect(
        isDeepStrictEqual(previewed.sent, selection) &&
          previewed.body?.status === "available" &&
          isDeepStrictEqual(previewed.body.profile, provider.profile),
      ).toBe(true);
      expect(
        (await read<Schema<"ArtistAssociation">[]>(associationsPath)).length,
      ).toBe(0);
      const confirmed = await preferencesAction<Schema<"ArtistAssociation">>(
        page,
        artist.getByRole("button", { name: "Use this artist", exact: true }),
        "POST",
        `${review}/confirm`,
        200,
      );
      if (!confirmed.body)
        throw new Error("The explicitly confirmed artist evidence is missing");
      const association = confirmed.body;
      expect(
        isDeepStrictEqual(confirmed.sent, selection) &&
          association.account_id === user.id &&
          association.recording_id === saved.ids[0] &&
          association.provider_artist_id === provider.profile.artist.id &&
          association.evidence === "user_confirmed" &&
          isDeepStrictEqual(association.provider, selected),
      ).toBe(true);
      const profile = artist.getByRole("region", {
        name: "Reviewed artist profile",
        exact: true,
      });
      await expect(
        profile.getByText(provider.profile.disambiguation, { exact: true }),
      ).toBeVisible();
      expect(
        provider.calls.filter((call) => call.startsWith("discography_")).length,
      ).toBe(0);

      stage = "post-confirmation read-only baseline";
      const baseline = await preferencesLibraryState(page, read, saved);
      const downloads = await artistReviewDownloadAbsence(page, saved.ids);
      const policy = await read<Schema<"MetadataSettings">>(
        "/api/v1/metadata/settings",
      );
      const connections =
        await read<Schema<"PluginSettings">>("/api/v1/plugins");
      expect(
        isDeepStrictEqual(await read(associationsPath), [association]),
      ).toBe(true);
      evidence.arm(
        association.id,
        editions
          ? ({
              release_group_id: editions.group.id,
              edition_id: editions.bonus.id,
            } satisfies Schema<"ReadEditionTrackList">)
          : undefined,
      );
      armed = true;
      const path = `${associationsPath}/${association.id}/discography`;

      stage = "explicit first discography page";
      const first = await preferencesAction<
        Schema<"ArtistDiscographyAvailability">
      >(
        page,
        profile.getByRole("button", {
          name: "Browse discography",
          exact: true,
        }),
        "POST",
        path,
        200,
      );
      if (first.body?.status !== "available" || !first.body.continuation)
        throw new Error("The owned first catalog page was unavailable");
      const continuation = first.body.continuation;
      privateValues.push(continuation);
      expect(
        isDeepStrictEqual(first.sent, {
          limit: 20,
          continuation: null,
        } satisfies Schema<"ReadArtistDiscography">) &&
          first.body.association_id === association.id &&
          isDeepStrictEqual(first.body.page, catalog.first) &&
          continuation !== catalog.cursor &&
          continuation.length <= 8192,
      ).toBe(true);
      const region = profile.getByRole("region", {
        name: "Discography",
        exact: true,
      });
      const releases = region.getByRole("list", {
        name: "Discography releases",
        exact: true,
      });
      await expect(releases.getByRole("listitem")).toHaveCount(1);
      await expect(
        releases.getByText(catalog.credit, { exact: true }),
      ).toBeVisible();
      await expect(releases.getByText("Live", { exact: true })).toBeVisible();
      await expect(
        releases.getByText("2024-02", { exact: true }),
      ).toBeVisible();

      stage = "host continuation preserves distinct same-title groups";
      const last = await preferencesAction<
        Schema<"ArtistDiscographyAvailability">
      >(
        page,
        region.getByRole("button", { name: "Load more releases", exact: true }),
        "POST",
        path,
        200,
      );
      expect(
        isDeepStrictEqual(last.sent, {
          limit: 20,
          continuation,
        } satisfies Schema<"ReadArtistDiscography">) &&
          last.body?.status === "available" &&
          last.body.association_id === association.id &&
          !last.body.continuation &&
          isDeepStrictEqual(last.body.page, catalog.last),
      ).toBe(true);
      await expect(releases.getByRole("listitem")).toHaveCount(2);
      await expect(
        releases.getByRole("heading", { name: catalog.title, exact: true }),
      ).toHaveCount(2);
      await expect(
        releases.getByText("Release", { exact: true }),
      ).toBeVisible();
      await expect(region.getByRole("link")).toHaveCount(0);
      await expect(region.getByRole("img")).toHaveCount(0);
      await expect(
        region.getByRole("button", {
          name: /download|import|add to library/i,
        }),
      ).toHaveCount(0);
      if (editions)
        await browseArtistEditions(
          page,
          releases,
          `${associationsPath}/${association.id}`,
          association.id,
          editions,
          provider.calls,
          privateValues,
          (next) => {
            stage = next;
          },
        );
      await expect(page.getByLabel("Track title", { exact: true })).toHaveValue(
        draft,
      );
      expect(
        isDeepStrictEqual(
          await preferencesLibraryState(page, read, saved),
          baseline,
        ),
      ).toBe(true);

      stage = "reload requires another explicit browse";
      await page.reload();
      await expect(page.getByLabel("Track title", { exact: true })).toHaveValue(
        saved.privateValues[0],
      );
      expect(
        provider.calls.filter((call) => call.startsWith("discography_")).length,
      ).toBe(2);
      await artist
        .getByRole("button", { name: "Find artist information", exact: true })
        .click();
      await artist
        .getByRole("button", {
          name: "View saved artist review 1",
          exact: true,
        })
        .click();
      await expect(
        profile.getByText(provider.profile.disambiguation, { exact: true }),
      ).toBeVisible();
      expect(
        provider.calls.filter((call) => call.startsWith("discography_")).length,
      ).toBe(2);
      const reloaded = await preferencesAction<
        Schema<"ArtistDiscographyAvailability">
      >(
        page,
        profile.getByRole("button", {
          name: "Browse discography",
          exact: true,
        }),
        "POST",
        path,
        200,
      );
      expect(
        isDeepStrictEqual(reloaded.sent, {
          limit: 20,
          continuation: null,
        } satisfies Schema<"ReadArtistDiscography">) &&
          reloaded.body?.status === "available" &&
          reloaded.body.association_id === association.id &&
          isDeepStrictEqual(reloaded.body.page, catalog.first) &&
          !!reloaded.body.continuation &&
          reloaded.body.continuation !== catalog.cursor &&
          reloaded.body.continuation.length <= 8192,
      ).toBe(true);
      if (reloaded.body?.status === "available" && reloaded.body.continuation)
        privateValues.push(reloaded.body.continuation);
      await expect(releases.getByRole("listitem")).toHaveCount(1);
      if (editions) {
        await expect(
          releases.getByRole("button", {
            name: "Browse editions",
            exact: true,
          }),
        ).toHaveCount(1);
        await expect(
          releases.getByRole("region", { name: "Editions", exact: true }),
        ).toHaveCount(0);
        expect(
          provider.calls.filter(
            (call) => call.startsWith("editions_") || call === "edition_tracks",
          ).length,
        ).toBe(6);
      }

      stage = "unchanged media collection authority and originals";
      expect(
        isDeepStrictEqual(
          await preferencesLibraryState(page, read, saved),
          baseline,
        ) &&
          isDeepStrictEqual(
            await artistReviewDownloadAbsence(page, saved.ids),
            downloads,
          ) &&
          isDeepStrictEqual(await artistReviewOriginals(), originals) &&
          isDeepStrictEqual(await read(associationsPath), [association]) &&
          isDeepStrictEqual(await read("/api/v1/metadata/settings"), policy) &&
          isDeepStrictEqual(await read("/api/v1/plugins"), connections) &&
          (
            await read<Schema<"ArtistAssociation">[]>(
              `/api/v1/recordings/${saved.ids[1]}/artist-associations`,
            )
          ).length === 0,
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
        isDeepStrictEqual(provider.calls, [
          "manifest_public",
          "manifest_private",
          "manifest_private",
          "discovery",
          "search",
          "profile_descriptor",
          "profile",
          "profile_descriptor",
          "profile",
          "profile_descriptor",
          "discography_first",
          "profile_descriptor",
          "discography_next",
          ...(editions
            ? [
                "editions_descriptor",
                "editions_first",
                "editions_descriptor",
                "editions_next",
                "editions_descriptor",
                "edition_tracks",
              ]
            : []),
          "profile_descriptor",
          "profile",
          "profile_descriptor",
          "discography_first",
        ]),
      ).toBe(true);
      stage = "bounded request and correlated log evidence";
      await expect.poll(() => evidence!.complete()).toBe(true);
      await expect.poll(() => evidence!.counts.discographyReplies).toBe(3);
      await expect
        .poll(() => evidence!.counts.editionReplies)
        .toBe(editionsEnabled ? 2 : 0);
      await expect
        .poll(() => evidence!.counts.trackListReplies)
        .toBe(editionsEnabled ? 1 : 0);
      await evidence.finish();
      const counts = evidence.counts;
      expect(
        counts.foreign +
          counts.audio +
          counts.download +
          counts.sourceWrites +
          counts.unexpectedWrites +
          counts.invalidDownloadStatus +
          counts.invalidCatalog +
          counts.privateConsole +
          counts.malformedLogs +
          counts.pageErrors +
          counts.bounds +
          counts.drainTimeout +
          provider.counters.unexpected +
          provider.counters.unsafeHeaders ===
          0 &&
          counts.discographyRequests === 3 &&
          counts.discographyReplies === 3 &&
          counts.editionRequests === (editionsEnabled ? 2 : 0) &&
          counts.editionReplies === (editionsEnabled ? 2 : 0) &&
          counts.trackListRequests === (editionsEnabled ? 1 : 0) &&
          counts.trackListReplies === (editionsEnabled ? 1 : 0) &&
          counts.downloadStatusReads === 2 &&
          counts.downloadStatusAbsentReplies === 2 &&
          counts.requests <= 300 &&
          counts.consoleBytes <= 524_288 &&
          provider.counters.requests === expectedProviderRequests,
      ).toBe(true);
    } catch {
      failed = true;
      failedBeforeArm = !armed;
    } finally {
      try {
        await context.close();
      } catch {
        failed = true;
      }
      try {
        await owned?.close();
      } catch {
        failed = true;
      }
      try {
        await evidence?.finish(true);
      } catch {
        failed = true;
      }
      checks = {
        ...(evidence?.counts ?? {}),
        providerRequests: owned?.counters.requests ?? 0,
        unexpected: owned?.counters.unexpected ?? 0,
        unsafeHeaders: owned?.counters.unsafeHeaders ?? 0,
        workflowsComplete: evidence?.complete() ?? false,
        publicReads,
        armed,
        failedBeforeArm,
      };
      if (
        evidence &&
        evidence.counts.foreign +
          evidence.counts.audio +
          evidence.counts.download +
          evidence.counts.sourceWrites +
          evidence.counts.unexpectedWrites +
          evidence.counts.invalidDownloadStatus +
          evidence.counts.invalidCatalog +
          evidence.counts.privateConsole +
          evidence.counts.malformedLogs +
          evidence.counts.pageErrors +
          evidence.counts.bounds +
          evidence.counts.drainTimeout +
          (owned?.counters.unexpected ?? 0) +
          (owned?.counters.unsafeHeaders ?? 0) >
          0
      )
        failed = true;
      if (
        evidence &&
        (!evidence.complete() ||
          evidence.counts.discographyRequests !== 3 ||
          evidence.counts.discographyReplies !== 3 ||
          evidence.counts.editionRequests !== (editionsEnabled ? 2 : 0) ||
          evidence.counts.editionReplies !== (editionsEnabled ? 2 : 0) ||
          evidence.counts.trackListRequests !== (editionsEnabled ? 1 : 0) ||
          evidence.counts.trackListReplies !== (editionsEnabled ? 1 : 0) ||
          evidence.counts.downloadStatusReads !== 2 ||
          evidence.counts.downloadStatusAbsentReplies !== 2 ||
          owned?.counters.requests !== expectedProviderRequests)
      )
        failed = true;
    }
    if (failed) {
      const summary = JSON.stringify(checks);
      if (Buffer.byteLength(summary) > 4096)
        throw new Error(
          "Discography browser verification exceeded its safe diagnostic bound",
        );
      throw new Error(
        `Discography browser verification failed at ${stage}; checks=${summary}`,
      );
    }
  },
);

async function insertMiddleRecording(
  page: Page,
  saved: Awaited<ReturnType<typeof seedPreferencesLibrary>>,
  user: Schema<"User">,
) {
  const middle = randomUUID();
  const batch = {
    device_id: "owned-discography-browser-preparation",
    operations: [
      {
        id: randomUUID(),
        action: {
          kind: "entry_move",
          playlist_id: saved.playlist,
          entry_id: saved.entries[1],
          position: 2,
        },
      },
      {
        id: randomUUID(),
        action: {
          kind: "entry_insert",
          playlist_id: saved.playlist,
          entry_id: middle,
          recording_id: saved.ids[1],
          position: 1,
        },
      },
    ],
  } satisfies Schema<"SyncBatch">;
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
    const bytes = await response.body();
    if (bytes.length > 65_536)
      throw new Error("Owned playlist preparation exceeded its response bound");
    const receipt = JSON.parse(bytes.toString()) as Schema<"SyncReceipt">;
    expect(
      response.status() === 200 &&
        isDeepStrictEqual(
          receipt.acknowledged,
          batch.operations.map((operation) => operation.id),
        ),
    ).toBe(true);
  } finally {
    await response.dispose();
  }
  return middle;
}
