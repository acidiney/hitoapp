import { randomUUID } from "node:crypto";
import { isDeepStrictEqual } from "node:util";
import { expect, test } from "@playwright/test";
import type { components } from "../src/generated";
import {
  artistReviewDownloadAbsence,
  artistReviewOriginals,
  artistReviewReader,
} from "./fixtures/artist-review-state";
import {
  preferencesAction,
  seedPreferencesLibrary,
} from "./fixtures/metadata-preferences-state";
import {
  recordingAttributionDeadline,
  recordingAttributionEvidence,
} from "./fixtures/recording-attribution-evidence";
import { recordingAttributionProvider } from "./fixtures/recording-attribution-provider";
import {
  connectRecordingProvider,
  discoverRecording,
  localRecordingState,
  openSavedRecording,
  retainedProviderState,
  verifyCredits,
} from "./fixtures/recording-attribution-state";

type Schema = components["schemas"];
test.use({
  trace: "off",
  screenshot: "off",
  video: "off",
  launchOptions: { args: [] },
});
test.skip(
  process.env.HITO_RECORDING_ATTRIBUTION_E2E !== "1",
  "Requires an opted-in fresh disposable recording attribution server",
);

test("saved recording credits retain ordered creators and immutable history without playing music", async ({
  page,
  context,
  baseURL,
}) => {
  test.setTimeout(120_000);
  page.setDefaultTimeout(10_000);
  page.setDefaultNavigationTimeout(15_000);
  let stage = 1;
  let cleaning = false;
  let failed = false;
  let provider: Awaited<
    ReturnType<typeof recordingAttributionProvider>
  > | null = null;
  let evidence: Awaited<
    ReturnType<typeof recordingAttributionEvidence>
  > | null = null;
  const checks: Record<string, number> = {};
  const username = "owned-credits-administrator";
  const password = `owned-credits-password-${randomUUID()}`;
  const query = `owned-credit-search-${randomUUID()}`;
  const privateValues = [
    username,
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
        process.env.PLAYWRIGHT_NO_COPY_PROMPT === "1",
    ).toBe(true);
    expect(
      (await read<Schema["SetupStatus"]>("/api/v1/setup")).needs_setup,
    ).toBe(true);
    evidence = await recordingAttributionEvidence(
      context,
      baseURL!,
      privateValues,
      () => (cleaning ? 9 : stage),
    );
    provider = await recordingAttributionProvider(query);
    const owned = provider;
    privateValues.push(...owned.privateValues);
    const originalsBefore = await artistReviewOriginals();
    await page.goto("/");
    await page.getByLabel("Username", { exact: true }).fill(username);
    await page.getByLabel("Password", { exact: true }).fill(password);
    await page
      .getByRole("button", { name: "Create administrator", exact: true })
      .click();
    await expect(
      page.getByRole("heading", { name: "Your library", exact: true }),
    ).toBeVisible();
    const user = await read<Schema["User"]>("/api/v1/me");
    expect(user.username === username && user.admin).toBe(true);
    privateValues.push(
      ...(await context.cookies()).map((cookie) => cookie.value),
    );
    evidence.armWorkflow();

    stage = 2;
    const saved = await seedPreferencesLibrary(page, read, user);
    privateValues.push(...saved.privateValues);
    const localBefore = await localRecordingState(page, read, saved);
    expect(
      localBefore.catalog.length === 2 &&
        localBefore.playlist.entries.length === 2 &&
        localBefore.playlist.entries.every(
          (entry) => entry.recording_id === saved.ids[0],
        ) &&
        isDeepStrictEqual(
          localBefore.playlist.entries.map((entry) => entry.entry_id),
          saved.entries,
        ) &&
        localBefore.metadata[0].title.personal === saved.privateValues[0] &&
        localBefore.metadata[0].artist.personal === saved.privateValues[1] &&
        localBefore.catalog.find((recording) => recording.id === saved.ids[0])
          ?.liked === true &&
        localBefore.progress.some(
          (progress) =>
            progress.recording_id === saved.ids[0] &&
            progress.position_ms === 400 &&
            progress.speed === 1.25,
        ) &&
        localBefore.sources.every((sources) => sources.length === 1),
    ).toBe(true);
    const localDownloads = await artistReviewDownloadAbsence(page, saved.ids);

    stage = 3;
    const connection = await connectRecordingProvider(page, owned);
    evidence.watchInstallation(connection.id);
    const first = await discoverRecording(
      page,
      query,
      connection,
      owned.original,
    );
    evidence.watchRecording(user.id, first.id);
    await openSavedRecording(page, first, owned.original.title);

    stage = 4;
    const selected = page.getByRole("region", {
      name: "Selected metadata credits",
      exact: true,
    });
    await verifyCredits(selected, owned.first);
    const history = page.getByRole("region", {
      name: "Source observations",
      exact: true,
    });
    await expect(history.getByRole("article")).toHaveCount(1);
    await verifyCredits(history.getByRole("article").first(), owned.first);
    const initial = await read<Schema["RecordingMetadata"]>(
      `/api/v1/recordings/${first.id}/metadata`,
    );
    expect(
      initial.recording_id === first.id &&
        initial.selected_observation?.origin === "provider_response" &&
        initial.selected_observation.provider_recording_id ===
          owned.original.id &&
        initial.observations.items.length === 1 &&
        isDeepStrictEqual(
          initial.selected_observation.attribution,
          owned.first,
        ),
    ).toBe(true);
    checks.initialCredits = 1;

    stage = 5;
    const choice = await read<Schema["SourceChoice"]>(
      `/api/v1/recordings/${first.id}/source-choice`,
    );
    expect(
      choice.mode === "automatic" &&
        choice.sources.length === 1 &&
        choice.sources[0].selectable &&
        !choice.sources[0].source.ready,
    ).toBe(true);
    const source = choice.sources[0].source;
    evidence.allowPin(source.id);
    const playback = page.getByRole("region", {
      name: "Playback source",
      exact: true,
    });
    await playback
      .getByRole("radio", { name: owned.source.label, exact: true })
      .check();
    const pinned = await preferencesAction<Schema["SourceChoiceReceipt"]>(
      page,
      playback.getByRole("button", { name: "Use source", exact: true }),
      "POST",
      `/api/v1/recordings/${first.id}/source-choice`,
      200,
    );
    expect(
      pinned.body?.source_id === source.id &&
        pinned.body.recording_id === first.id,
    ).toBe(true);
    await expect(
      playback.getByText("Manual source selection", { exact: true }),
    ).toBeVisible();
    const providerBefore = await retainedProviderState(read, first.id);
    expect(
      providerBefore.choice.mode === "manual" &&
        providerBefore.choice.sources[0].selected &&
        providerBefore.sources.length === 1 &&
        !providerBefore.sources[0].ready,
    ).toBe(true);

    stage = 6;
    owned.revise();
    expect(
      isDeepStrictEqual(
        { ...owned.replacement, attribution: owned.first },
        owned.original,
      ),
    ).toBe(true);
    const repeated = await discoverRecording(
      page,
      query,
      connection,
      owned.replacement,
    );
    expect(
      repeated.id === first.id && repeated.library_id === first.library_id,
    ).toBe(true);
    await openSavedRecording(page, repeated, owned.original.title);
    await verifyCredits(selected, owned.second);
    await expect(history.getByRole("article")).toHaveCount(2);
    const articles = history.getByRole("article");
    await verifyCredits(articles.nth(0), owned.second);
    await verifyCredits(articles.nth(1), owned.first);
    await expect(
      articles.nth(0).getByText("Current metadata", { exact: true }),
    ).toBeVisible();
    await expect(
      articles.nth(1).getByText("Current metadata", { exact: true }),
    ).toHaveCount(0);
    const updated = await read<Schema["RecordingMetadata"]>(
      `/api/v1/recordings/${first.id}/metadata`,
    );
    expect(
      updated.recording_id === first.id &&
        updated.selected_observation?.id !== initial.selected_observation?.id &&
        updated.observations.items.length === 2 &&
        isDeepStrictEqual(
          updated.selected_observation?.attribution,
          owned.second,
        ) &&
        isDeepStrictEqual(
          updated.observations.items[1],
          initial.selected_observation,
        ),
    ).toBe(true);
    checks.retainedObservations = 2;
    const callsAfterAdoption = structuredClone(owned.calls);

    stage = 7;
    await page.reload();
    await verifyCredits(selected, owned.second);
    await verifyCredits(history.getByRole("article").nth(1), owned.first);
    expect(
      isDeepStrictEqual(
        await read(`/api/v1/recordings/${first.id}/metadata`),
        updated,
      ),
    ).toBe(true);
    const catalog = await read<Schema["RecordingPage"]>("/api/v1/recordings");
    expect(
      catalog.total === 3 &&
        catalog.items.length === 3 &&
        isDeepStrictEqual(
          new Set(catalog.items.map((recording) => recording.id)),
          new Set([...saved.ids, first.id]),
        ),
    ).toBe(true);
    expect(
      isDeepStrictEqual(
        await localRecordingState(page, read, saved),
        localBefore,
      ) &&
        isDeepStrictEqual(
          await retainedProviderState(read, first.id),
          providerBefore,
        ) &&
        isDeepStrictEqual(await artistReviewOriginals(), originalsBefore) &&
        isDeepStrictEqual(
          await artistReviewDownloadAbsence(page, saved.ids),
          localDownloads,
        ),
    ).toBe(true);
    await artistReviewDownloadAbsence(page, [first.id]);
    expect(
      (await page
        .locator("audio[data-hito-player]")
        .evaluate(
          (audio: HTMLAudioElement) =>
            audio.paused && audio.currentTime === 0 && !audio.currentSrc,
        )) &&
        (await page.evaluate(
          () =>
            !("ownedCreditPayload" in window) &&
            !("ownedCreditProvider" in window),
        )),
    ).toBe(true);
    expect(
      isDeepStrictEqual(owned.calls, callsAfterAdoption) &&
        isDeepStrictEqual(owned.calls, {
          requests: 8,
          publicManifest: 1,
          privateManifest: 1,
          search: 2,
          detail: 2,
          sources: 2,
          unexpected: 0,
          unsafeHeaders: 0,
        }),
    ).toBe(true);
    checks.preservedState = 1;
    stage = 8;
    await evidence.finish();
    expect(
      evidence.counts.foreign +
        evidence.counts.forbidden +
        evidence.counts.audio +
        evidence.counts.preparation +
        evidence.counts.download +
        evidence.counts.invalidStatus +
        evidence.counts.pageErrors +
        evidence.counts.terminalRequestFailures +
        evidence.counts.failureRecordsDropped +
        evidence.counts.privateConsole +
        evidence.counts.captureFailures +
        evidence.counts.drainTimeouts ===
        0 &&
        evidence.counts.workflowArmed === 1 &&
        evidence.counts.requestFailures ===
          evidence.counts.preparationCancellations &&
        evidence.counts.requests <= 250 &&
        evidence.counts.searches === 2 &&
        evidence.counts.adoptions === 2 &&
        evidence.counts.pins === 1 &&
        evidence.counts.statusReads >= 3 &&
        evidence.counts.statusReads === evidence.counts.absentReplies &&
        evidence.counts.consoleBytes <= 524_288 &&
        evidence.counts.text > 0 &&
        evidence.counts.snapshots > 0,
    ).toBe(true);
  } catch {
    failed = true;
  } finally {
    cleaning = true;
    async function cleanup(
      key: string,
      action: () => Promise<unknown>,
      milliseconds = 5000,
    ) {
      checks[key] = 0;
      try {
        await recordingAttributionDeadline(action(), milliseconds);
      } catch {
        checks[key] = 1;
        failed = true;
      }
    }
    await cleanup("cookieCleanupFailures", async () => {
      privateValues.push(
        ...(await context.cookies()).map((cookie) => cookie.value),
      );
    });
    await cleanup(
      "pageCleanupFailures",
      async () => {
        const results = await Promise.allSettled(
          context
            .pages()
            .map((active) => active.goto("about:blank", { timeout: 5000 })),
        );
        if (results.some((result) => result.status === "rejected"))
          throw new Error("Owned recording pages did not stop");
      },
      6000,
    );
    await cleanup("routeCleanupFailures", async () => {
      if (
        context
          .pages()
          .some(
            (active) => !active.isClosed() && active.url() !== "about:blank",
          )
      )
        throw new Error("Owned recording routes still protect a live page");
      await context.unrouteAll({ behavior: "wait" });
    });
    checks.browserCloseAttempted = 1;
    await cleanup("browserCleanupFailures", () => context.close());
    checks.providerCloseAttempted = provider ? 1 : 0;
    await cleanup(
      "providerCleanupFailures",
      async () => {
        await provider?.close();
      },
      6000,
    );
    await cleanup(
      "drainCleanupFailures",
      async () => {
        await evidence?.finish();
      },
      6000,
    );
    if (
      evidence &&
      (evidence.counts.foreign +
        evidence.counts.forbidden +
        evidence.counts.audio +
        evidence.counts.preparation +
        evidence.counts.download +
        evidence.counts.invalidStatus +
        evidence.counts.pageErrors +
        evidence.counts.terminalRequestFailures +
        evidence.counts.failureRecordsDropped +
        evidence.counts.privateConsole +
        evidence.counts.captureFailures +
        evidence.counts.drainTimeouts >
        0 ||
        evidence.counts.workflowArmed !== 1 ||
        evidence.counts.requestFailures !==
          evidence.counts.preparationCancellations ||
        evidence.counts.statusReads !== evidence.counts.absentReplies)
    )
      failed = true;
    if (provider && (provider.calls.unexpected || provider.calls.unsafeHeaders))
      failed = true;
  }
  const report = JSON.stringify({
    stage,
    ...checks,
    ...evidence?.counts,
    failures: evidence?.failures ?? [],
    providerRequests: provider?.calls.requests ?? 0,
    providerUnexpected: provider?.calls.unexpected ?? 0,
    providerUnsafeHeaders: provider?.calls.unsafeHeaders ?? 0,
  });
  if (Buffer.byteLength(report) > 3800)
    throw new Error("Recording attribution browser report exceeded its bound");
  if (failed)
    throw new Error(
      `Recording attribution browser verification failed; counts=${report}`,
    );
  console.info(`Recording attribution browser counts=${report}`);
});
