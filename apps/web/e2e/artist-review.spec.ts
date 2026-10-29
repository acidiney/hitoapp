import { randomUUID } from "node:crypto";
import { isDeepStrictEqual } from "node:util";
import { expect, test } from "@playwright/test";
import type { components } from "../src/generated";
import { reviewPluginInstallation } from "./fixtures/plugin-installation-review";
import { artistReviewEvidence } from "./fixtures/artist-review-evidence";
import { artistReviewProvider } from "./fixtures/artist-review-provider";
import {
  artistReviewDownloadAbsence,
  artistReviewOriginals,
  artistReviewReader,
} from "./fixtures/artist-review-state";
import {
  openPreferences,
  preferencesAction,
  preferencesLibraryState,
  savePreferences,
  seedPreferencesLibrary,
} from "./fixtures/metadata-preferences-state";

type Schema = components["schemas"];
test.use({
  trace: "off",
  screenshot: "off",
  video: "off",
  launchOptions: { args: [] },
});
test.skip(
  process.env.HITO_ARTIST_REVIEW_E2E !== "1",
  "Requires an opted-in fresh disposable artist review server",
);

test("an account reviews a namesake through its chosen metadata provider without changing or playing music", async ({
  page,
  context,
  baseURL,
}) => {
  test.setTimeout(120_000);
  page.setDefaultTimeout(10_000);
  page.setDefaultNavigationTimeout(15_000);
  let stage = "fresh disposable server";
  let checks: Record<string, boolean | number> = {};
  let failed = false;
  let provider: Awaited<ReturnType<typeof artistReviewProvider>> | null = null;
  let evidence: Awaited<ReturnType<typeof artistReviewEvidence>> | null = null;
  const password = "owned artist browser administrator password";
  const query = `Private edited namesake query ${randomUUID()}`;
  const draft = `Private unsaved track title ${randomUUID()}`;
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
  const read = artistReviewReader(page);
  try {
    expect(baseURL === "http://localhost:8080").toBe(true);
    expect(
      (await read<Schema["SetupStatus"]>("/api/v1/setup")).needs_setup,
    ).toBe(true);
    evidence = await artistReviewEvidence(context, baseURL!, privateValues);
    provider = await artistReviewProvider(query);
    const owned = provider;
    privateValues.push(
      owned.token,
      owned.origin,
      owned.manifest.name,
      owned.manifest.id,
      ...owned.candidates.flatMap((candidate) => [
        candidate.artist.id,
        candidate.artist.name,
        candidate.disambiguation,
      ]),
      owned.profile.revision,
      owned.profile.disambiguation,
    );
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

    stage = "owned originals and private collection state";
    const saved = await seedPreferencesLibrary(page, read, user);
    privateValues.push(...saved.privateValues);
    evidence.watchDownloadStatus(user.id, saved.ids[0]);
    const musicBefore = await preferencesLibraryState(page, read, saved);
    const downloadsBefore = await artistReviewDownloadAbsence(page, saved.ids);
    expect(
      musicBefore.playlist.entries.length === 2 &&
        musicBefore.playlist.entries.every(
          (entry) => entry.recording_id === saved.ids[0],
        ) &&
        isDeepStrictEqual(
          musicBefore.playlist.entries.map((entry) => entry.entry_id),
          saved.entries,
        ) &&
        musicBefore.metadata[0].title.personal === saved.privateValues[0] &&
        musicBefore.metadata[0].artist.personal === saved.privateValues[1] &&
        musicBefore.catalog.items.find((item) => item.id === saved.ids[0])
          ?.liked === true &&
        musicBefore.progress.some(
          (item) =>
            item.recording_id === saved.ids[0] &&
            item.position_ms === 400 &&
            item.speed === 1.25,
        ) &&
        musicBefore.sources.every((sources) => sources.length === 1),
    ).toBe(true);

    stage = "address-only service offer and personal provider approval";
    await page.getByRole("tab", { name: "Plugins", exact: true }).click();
    await page.getByText("Services for this server", { exact: true }).click();
    await page
      .getByLabel("Service address", { exact: true })
      .fill(owned.address);
    const offered = await preferencesAction<Schema["PluginService"]>(
      page,
      page.getByRole("button", { name: "Offer service", exact: true }),
      "POST",
      "/api/v1/plugins/services",
      201,
    );
    expect(
      isDeepStrictEqual(offered.sent, { base_url: owned.address }) &&
        offered.body?.name === owned.manifest.name,
    ).toBe(true);
    if (!offered.body)
      throw new Error("The offered artist provider is missing");
    await page
      .getByRole("combobox", { name: "Service", exact: true })
      .selectOption(offered.body.id);
    await page
      .getByLabel("Bearer token (optional)", { exact: true })
      .fill(owned.token);
    const reviewed = await reviewPluginInstallation(
      page,
      offered.body,
      owned.manifest.name,
    );
    const installed = await preferencesAction<Schema["PluginInstallation"]>(
      page,
      page.getByRole("button", { name: "Accept and connect", exact: true }),
      "POST",
      "/api/v1/plugins",
      201,
    );
    if (!installed.body)
      throw new Error("The approved artist provider is missing");
    expect(
      installed.body.service_id === offered.body.id &&
        installed.body.credential_set &&
        isDeepStrictEqual(installed.body.manifest, owned.manifest) &&
        isDeepStrictEqual(installed.sent, {
          ...reviewed.sent,
          accept_manifest_digest: reviewed.body.digest,
        }),
    ).toBe(true);
    const selected = {
      kind: "http_plugin",
      installation_id: installed.body.id,
    } satisfies Schema["MetadataProvider"];
    expect(
      isDeepStrictEqual(owned.calls, [
        "manifest_public",
        "manifest_private",
        "manifest_private",
      ]),
    ).toBe(true);

    stage = "choose personal metadata provider instead of built-in";
    const panel = await openPreferences(page);
    await panel
      .getByRole("button", { name: "Remove MusicBrainz", exact: true })
      .click();
    await panel
      .getByRole("combobox", { name: "Metadata provider", exact: true })
      .selectOption(installed.body.id);
    await panel
      .getByRole("button", { name: "Add metadata provider", exact: true })
      .click();
    await savePreferences(page, {
      revision: 0,
      enabled: true,
      providers: [selected],
    });
    const policy = await read<Schema["MetadataSettings"]>(
      "/api/v1/metadata/settings",
    );
    expect(
      isDeepStrictEqual(policy, {
        revision: 1,
        enabled: true,
        providers: [selected],
      }),
    ).toBe(true);
    const connections = await read<Schema["PluginSettings"]>("/api/v1/plugins");

    stage = "navigate to actual recording details and retain unsaved draft";
    const navigation = page.getByRole("navigation", {
      name: "Main navigation",
      exact: true,
    });
    await navigation
      .getByRole("link", { name: "Biblioteca", exact: true })
      .click();
    await page
      .getByRole("link", {
        name: `Details for ${saved.privateValues[0]}`,
        exact: true,
      })
      .click();
    await expect(
      page.getByRole("heading", { name: "Your track details", exact: true }),
    ).toBeVisible();
    await page.getByLabel("Track title", { exact: true }).fill(draft);
    const artist = page.getByRole("region", {
      name: "Artist information",
      exact: true,
    });
    const associationsPath = `/api/v1/recordings/${saved.ids[0]}/artist-associations`;
    const reviewPath = `/api/v1/recordings/${saved.ids[0]}/artist-review`;
    expect(
      (await read<Schema["ArtistAssociation"][]>(associationsPath)).length,
    ).toBe(0);
    await artist
      .getByRole("button", { name: "Find artist information", exact: true })
      .click();
    await expect(
      artist.getByRole("combobox", { name: "Metadata provider", exact: true }),
    ).toHaveValue(installed.body.id);
    await artist
      .getByRole("searchbox", { name: "Search artists", exact: true })
      .fill(query);
    expect(
      isDeepStrictEqual(owned.calls, [
        "manifest_public",
        "manifest_private",
        "manifest_private",
      ]),
    ).toBe(true);

    stage = "search namesakes through negotiated discovery protocol";
    const searched = await preferencesAction<
      Schema["ArtistCandidateSearchAvailability"]
    >(
      page,
      artist.getByRole("button", { name: "Search artists", exact: true }),
      "POST",
      `${reviewPath}/search`,
      200,
    );
    if (searched.body?.status !== "available")
      throw new Error("The artist candidates were unavailable");
    const reviewContext = searched.body.context;
    expect(
      reviewContext.account_id === user.id &&
        reviewContext.recording_id === saved.ids[0] &&
        reviewContext.credited_name === saved.privateValues[1] &&
        reviewContext.observation_id ===
          musicBefore.metadata[0].selected_observation?.id &&
        reviewContext.source_id ===
          (musicBefore.metadata[0].selected_observation?.source_id ?? null) &&
        isDeepStrictEqual(reviewContext.provider, selected) &&
        isDeepStrictEqual(searched.sent, {
          context: reviewContext,
          search: owned.search,
        }) &&
        isDeepStrictEqual(searched.body.page.items, owned.candidates),
    ).toBe(true);
    const results = artist.getByRole("list", {
      name: "Artist search results",
      exact: true,
    });
    await expect(results.getByRole("listitem")).toHaveCount(2);
    const second = results
      .getByRole("listitem")
      .filter({ hasText: owned.candidates[1].disambiguation });
    await expect(
      second.getByRole("heading", {
        name: owned.candidates[1].artist.name,
        exact: true,
      }),
    ).toBeVisible();
    expect(
      (await read<Schema["ArtistAssociation"][]>(associationsPath)).length,
    ).toBe(0);

    stage = "preview exact second opaque identity without saving";
    const selection = {
      context: reviewContext,
      provider_artist_id: owned.profile.artist.id,
    } satisfies Schema["SelectArtistCandidate"];
    const previewed = await preferencesAction<
      Schema["ArtistCandidatePreviewAvailability"]
    >(
      page,
      second.getByRole("button", { name: "Review artist", exact: true }),
      "POST",
      `${reviewPath}/preview`,
      200,
    );
    expect(
      isDeepStrictEqual(previewed.sent, selection) &&
        previewed.body?.status === "available" &&
        isDeepStrictEqual(previewed.body.profile, owned.profile),
    ).toBe(true);
    await expect(
      artist
        .getByRole("region", { name: "Review artist", exact: true })
        .getByText(owned.profile.disambiguation, { exact: true }),
    ).toBeVisible();
    expect(
      (await read<Schema["ArtistAssociation"][]>(associationsPath)).length,
    ).toBe(0);
    expect(
      isDeepStrictEqual(
        await preferencesLibraryState(page, read, saved),
        musicBefore,
      ),
    ).toBe(true);
    await expect(page.getByLabel("Track title", { exact: true })).toHaveValue(
      draft,
    );
    expect(
      isDeepStrictEqual(owned.calls, [
        "manifest_public",
        "manifest_private",
        "manifest_private",
        "discovery",
        "search",
        "profile_descriptor",
        "profile",
      ]),
    ).toBe(true);

    stage = "explicit confirmation and reviewed profile";
    const confirmed = await preferencesAction<Schema["ArtistAssociation"]>(
      page,
      artist.getByRole("button", { name: "Use this artist", exact: true }),
      "POST",
      `${reviewPath}/confirm`,
      200,
    );
    expect(
      isDeepStrictEqual(confirmed.sent, selection) &&
        confirmed.body?.provider_artist_id === owned.profile.artist.id &&
        confirmed.body.account_id === user.id &&
        confirmed.body.recording_id === saved.ids[0] &&
        confirmed.body.evidence === "user_confirmed" &&
        isDeepStrictEqual(confirmed.body.provider, selected),
    ).toBe(true);
    await expect(
      artist.getByRole("status").filter({ hasText: "Artist review saved." }),
    ).toBeVisible();
    await expect(
      artist
        .getByRole("region", { name: "Reviewed artist profile", exact: true })
        .getByText(owned.profile.disambiguation, { exact: true }),
    ).toBeVisible();
    await expect(page.getByLabel("Track title", { exact: true })).toHaveValue(
      draft,
    );
    const associations =
      await read<Schema["ArtistAssociation"][]>(associationsPath);
    expect(isDeepStrictEqual(associations, [confirmed.body])).toBe(true);
    expect(
      (
        await read<Schema["ArtistAssociation"][]>(
          `/api/v1/recordings/${saved.ids[1]}/artist-associations`,
        )
      ).length,
    ).toBe(0);

    stage = "reload retains saved review and explicitly opens its profile";
    await page.reload();
    await expect(page.getByLabel("Track title", { exact: true })).toHaveValue(
      saved.privateValues[0],
    );
    await artist
      .getByRole("button", { name: "Find artist information", exact: true })
      .click();
    await artist
      .getByRole("button", { name: "View saved artist review 1", exact: true })
      .click();
    await expect(
      artist
        .getByRole("region", { name: "Reviewed artist profile", exact: true })
        .getByText(owned.profile.disambiguation, { exact: true }),
    ).toBeVisible();
    expect(isDeepStrictEqual(await read(associationsPath), associations)).toBe(
      true,
    );

    stage =
      "unchanged originals, private music, provider policy and credentials";
    const musicAfter = await preferencesLibraryState(page, read, saved);
    checks = Object.fromEntries(
      (Object.keys(musicBefore) as (keyof typeof musicBefore)[]).map((key) => [
        key,
        isDeepStrictEqual(musicAfter[key], musicBefore[key]),
      ]),
    );
    expect(isDeepStrictEqual(musicAfter, musicBefore)).toBe(true);
    expect(
      isDeepStrictEqual(
        await artistReviewDownloadAbsence(page, saved.ids),
        downloadsBefore,
      ) &&
        isDeepStrictEqual(await artistReviewOriginals(), originalsBefore) &&
        isDeepStrictEqual(await read("/api/v1/metadata/settings"), policy) &&
        isDeepStrictEqual(await read("/api/v1/plugins"), connections),
    ).toBe(true);
    expect(
      await page
        .locator("audio[data-hito-player]")
        .evaluate(
          (audio: HTMLAudioElement) =>
            audio.paused && audio.currentTime === 0 && !audio.currentSrc,
        ),
    ).toBe(true);

    stage =
      "bounded owned protocol, navigation and safe correlated workflow logs";
    await expect.poll(() => evidence!.accepted.profile).toBe(2);
    await evidence.finish();
    checks = {
      ...evidence.counts,
      providerRequests: owned.counters.requests,
      unexpected: owned.counters.unexpected,
      unsafeHeaders: owned.counters.unsafeHeaders,
      workflowsComplete: evidence.complete(),
    };
    expect(
      isDeepStrictEqual(owned.calls, [
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
        "profile",
      ]),
    ).toBe(true);
    const counts = evidence.counts;
    expect(
      counts.foreign +
        counts.audio +
        counts.download +
        counts.invalidDownloadStatus +
        counts.sourceWrites +
        counts.pageErrors +
        counts.privateConsole +
        counts.malformedLogs +
        owned.counters.unexpected +
        owned.counters.unsafeHeaders ===
        0 &&
        counts.requests <= 250 &&
        counts.consoleBytes <= 524_288 &&
        evidence.complete() &&
        counts.recordingNavigations === 2 &&
        counts.downloadStatusReads === 2 &&
        counts.downloadStatusAbsentReplies === 2 &&
        counts.context === 1 &&
        counts.search === 1 &&
        counts.preview === 1 &&
        counts.confirm === 1 &&
        counts.profile === 2 &&
        isDeepStrictEqual(evidence.accepted, {
          load: 2,
          search: 1,
          preview: 1,
          confirm: 1,
          profile: 2,
        }),
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
    await evidence?.finish();
    if (
      evidence &&
      (evidence.counts.privateConsole || evidence.counts.malformedLogs)
    )
      failed = true;
  }
  if (failed)
    throw new Error(
      `Artist review browser verification failed at ${stage}; checks=${JSON.stringify(checks)}`,
    );
});
