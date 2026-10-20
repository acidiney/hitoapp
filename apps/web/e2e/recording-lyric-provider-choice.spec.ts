import { randomUUID } from "node:crypto";
import { writeFile } from "node:fs/promises";
import { isDeepStrictEqual } from "node:util";
import { expect, request, test } from "@playwright/test";
import type { components } from "../src/generated";
import { artistReviewReader } from "./fixtures/artist-review-state";
import { seedPreferencesLibrary } from "./fixtures/metadata-preferences-state";
import {
  discoverRecording,
  openSavedRecording,
  verifyCredits,
} from "./fixtures/recording-attribution-state";
import {
  assertLyricReply,
  lyricDeadline,
  lyricOriginals,
  lyricState,
} from "./fixtures/recording-lyrics-state";
import { lyricChoiceProviders } from "./fixtures/recording-lyric-provider-choice-provider";
import { lyricChoiceEvidence } from "./fixtures/recording-lyric-provider-choice-evidence";
import {
  choicePeer,
  choicePeerState,
  choiceRead,
  choiceWrite,
  connectChoiceProvider,
} from "./fixtures/recording-lyric-provider-choice-state";

type Schema = components["schemas"];
test.skip(
  process.env.HITO_LYRIC_PROVIDER_CHOICE_E2E !== "1",
  "Requires the disposable packaged provider-choice gate",
);

test("explicit choice of a different retained provider renders its exact lyrics without changing metadata or private state", async ({
  page,
  context,
  baseURL,
  browserName,
}) => {
  test.setTimeout(120_000);
  page.setDefaultTimeout(10_000);
  page.setDefaultNavigationTimeout(15_000);
  const credentials = {
    username: `choice-owner-${randomUUID().slice(0, 8)}`,
    password: `owned-choice-password-${randomUUID()}`,
  } satisfies Schema["Credentials"];
  const query = `owned-choice-query-${randomUUID()}`;
  const draft = `Owned unsaved choice title ${randomUUID()}`;
  const privateValues = [
    credentials.username,
    credentials.password,
    query,
    draft,
    "/music/",
    "Fixture 440 Hz",
    "Fixture 660 Hz",
    "Hito test fixtures",
  ];
  const checks: Record<string, boolean> = {};
  let checkpoint = 1;
  let failed = false;
  let providers: Awaited<ReturnType<typeof lyricChoiceProviders>> | undefined;
  let evidence: Awaited<ReturnType<typeof lyricChoiceEvidence>> | undefined;
  let browserEvidence:
    | Awaited<
        ReturnType<Awaited<ReturnType<typeof lyricChoiceEvidence>>["finish"]>
      >
    | undefined;
  const peer = await request.newContext({ baseURL });
  try {
    expect(
      baseURL === "http://localhost:8080" &&
        process.env.PLAYWRIGHT_NO_COPY_PROMPT === "1",
    ).toBe(true);
    expect(
      (await choiceRead<Schema["SetupStatus"]>(page.request, "/api/v1/setup"))
        .needs_setup,
    ).toBe(true);
    evidence = await lyricChoiceEvidence(context, baseURL!, privateValues);
    await choiceWrite<Schema["AuthSession"]>(
      page.request,
      baseURL!,
      "/api/v1/setup",
      credentials,
      201,
    );
    privateValues.push(
      ...(await context.cookies()).map((cookie) => cookie.value),
    );
    await page.goto("/library");
    await expect(
      page.getByRole("heading", { name: "Your library", exact: true }),
    ).toBeVisible();
    const user = await choiceRead<Schema["User"]>(page.request, "/api/v1/me");
    expect(user.admin && user.username === credentials.username).toBe(true);
    providers = await lyricChoiceProviders(query);
    privateValues.push(...providers.privateValues);
    const originals = await lyricOriginals();

    checkpoint = 2;
    const local = await seedPreferencesLibrary(
      page,
      artistReviewReader(page),
      user,
    );
    privateValues.push(...local.privateValues);
    evidence.own(local.ids);
    const peerPlaylist = await choicePeer(
      page.request,
      peer,
      baseURL!,
      privateValues,
    );
    const primary = await connectChoiceProvider(page, providers.primary);
    const attached = await connectChoiceProvider(page, providers.attached);
    expect(
      primary.id !== attached.id &&
        primary.manifest.capabilities.includes("lyrics") &&
        attached.manifest.capabilities.includes("lyrics"),
    ).toBe(true);
    checkpoint = 3;
    const recording = await discoverRecording(
      page,
      query,
      primary,
      providers.primary.recording,
    );
    evidence.own([recording.id]);
    const primaryMetadata = await choiceRead<Schema["RecordingMetadata"]>(
      page.request,
      `/api/v1/recordings/${recording.id}/metadata`,
    );
    const primaryObservation = primaryMetadata.selected_observation;
    expect(
      primaryObservation?.plugin_installation_id === primary.id &&
        primaryObservation.provider_recording_id ===
          providers.primary.recording.id &&
        primaryObservation.provider_revision ===
          providers.primary.recording.revision,
    ).toBe(true);
    if (!primaryObservation)
      throw new Error("Owned primary observation is absent");

    checkpoint = 4;
    const resolutionRequest = {
      request_id: randomUUID(),
    } satisfies Schema["ResolveSource"];
    const resolution = await choiceWrite<Schema["SourceResolution"]>(
      page.request,
      baseURL!,
      `/api/v1/recordings/${recording.id}/sources/resolve`,
      resolutionRequest,
    );
    expect(
      resolution.outcome === "ready" &&
        resolution.recording_id === recording.id &&
        resolution.request_id === resolutionRequest.request_id &&
        resolution.sources.length === 1 &&
        resolution.sources.every(
          (source) => source.provider === "plugin" && source.ready,
        ),
    ).toBe(true);
    expect(
      providers.attached.calls.search > 0 &&
        providers.attached.calls.detail > 0 &&
        providers.attached.calls.sources > 0,
    ).toBe(true);
    const metadata = await choiceRead<Schema["RecordingMetadata"]>(
      page.request,
      `/api/v1/recordings/${recording.id}/metadata`,
    );
    expect(
      isDeepStrictEqual(metadata.selected_observation, primaryObservation) &&
        isDeepStrictEqual(metadata.title, primaryMetadata.title) &&
        isDeepStrictEqual(metadata.artist, primaryMetadata.artist),
    ).toBe(true);
    const attachedObservations = metadata.observations.items.filter(
      (item) =>
        item.plugin_installation_id === attached.id &&
        item.provider_recording_id === providers!.attached.recording.id &&
        item.provider_revision === providers!.attached.recording.revision &&
        item.origin === "provider_response" &&
        item.source_id === null,
    );
    expect(
      attachedObservations.length === 1 &&
        attachedObservations[0].id !== primaryObservation.id,
    ).toBe(true);
    const attachedObservation = attachedObservations[0];
    privateValues.push(primaryObservation.id, attachedObservation.id);
    checks.publicRetainedIdentities = true;

    checkpoint = 5;
    await openSavedRecording(
      page,
      recording,
      providers.primary.recording.title,
    );
    await page.waitForLoadState("networkidle");
    const ids = [...local.ids, recording.id];
    const before = await lyricState(page, ids, local.playlist);
    const peerBefore = await choicePeerState(peer, peerPlaylist);
    expect(
      before.catalog.total === 3 &&
        before.playlist.entries.length === 2 &&
        before.progress.some(
          (item) =>
            item.recording_id === local.ids[0] &&
            item.position_ms === 400 &&
            item.speed === 1.25,
        ) &&
        peerBefore.playlist.entries.length === 0 &&
        peerBefore.playlist.playlist.name.length > 0,
    ).toBe(true);
    const providerBefore = {
      primary: { ...providers.primary.calls },
      attached: { ...providers.attached.calls },
    };
    expect(
      providerBefore.primary.lyrics +
        providerBefore.attached.lyrics +
        providerBefore.primary.descriptor +
        providerBefore.attached.descriptor +
        providerBefore.primary.audio +
        providerBefore.attached.audio ===
        0,
    ).toBe(true);
    const lyrics = page.getByRole("region", { name: "Lyrics", exact: true });
    await expect(
      lyrics.getByRole("button", {
        name: "Choose lyric provider",
        exact: true,
      }),
    ).toBeVisible();
    expect(evidence.counts.lists === 0 && evidence.counts.lyrics === 0).toBe(
      true,
    );
    const input = page.getByRole("textbox", {
      name: "Track title",
      exact: true,
    });
    await input.fill(draft);
    const editor = await input.elementHandle();
    if (!editor) throw new Error("Owned metadata editor is absent");
    const document = await page.evaluate(() => performance.timeOrigin);
    evidence.arm(recording.id);
    providers.primary.arm();
    providers.attached.arm();

    checkpoint = 6;
    const [listReply] = await Promise.all([
      page.waitForResponse(
        (reply) =>
          reply.request().method() === "GET" &&
          reply.url() ===
            `${baseURL}/api/v1/recordings/${recording.id}/lyrics/providers`,
      ),
      lyrics
        .getByRole("button", { name: "Choose lyric provider", exact: true })
        .click(),
    ]);
    const bytes = await listReply.body();
    const expected = {
      recording_id: recording.id,
      items: [
        {
          observation_id: primaryObservation.id,
          installation_id: primary.id,
          installation_revision: primary.revision,
          provider_recording: providers.primary.reference,
          label: providers.primary.manifest.name,
        },
        {
          observation_id: attachedObservation.id,
          installation_id: attached.id,
          installation_revision: attached.revision,
          provider_recording: providers.attached.reference,
          label: providers.attached.manifest.name,
        },
      ],
    } satisfies Schema["RecordingLyricsProviders"];
    expect(
      listReply.status() === 200 &&
        listReply.headers()["cache-control"] === "private, no-store" &&
        bytes.length <= 16_384 &&
        isDeepStrictEqual(JSON.parse(bytes.toString()), expected),
    ).toBe(true);
    const selector = lyrics.getByRole("combobox", {
      name: "Lyric provider",
      exact: true,
    });
    expect(
      isDeepStrictEqual(
        await selector.getByRole("option").allTextContents(),
        expected.items.map((item) => item.label),
      ),
    ).toBe(true);
    await expect(selector).toHaveValue(primaryObservation.id);
    checks.currentListExact = true;

    checkpoint = 7;
    await selector.selectOption(attachedObservation.id);
    await expect(selector).toHaveValue(attachedObservation.id);
    expect(
      evidence.counts.lists === 1 &&
        evidence.counts.lyrics === 0 &&
        isDeepStrictEqual(providers.primary.calls, providerBefore.primary) &&
        isDeepStrictEqual(providers.attached.calls, providerBefore.attached),
    ).toBe(true);
    checks.choiceDoesNotLookup = true;

    checkpoint = 8;
    const result = await assertLyricReply(
      page,
      recording.id,
      attachedObservation.id,
      attached.revision,
      () =>
        lyrics
          .getByRole("button", { name: "Show lyrics", exact: true })
          .click(),
    );
    expect(
      isDeepStrictEqual(result, {
        status: "ready",
        recording_id: recording.id,
        observation_id: attachedObservation.id,
        document: {
          recording: providers.attached.reference,
          lyrics: providers.attached.lyric,
        },
      }),
    ).toBe(true);
    const lines = lyrics.getByRole("list", {
      name: "Lyric lines",
      exact: true,
    });
    expect(
      isDeepStrictEqual(
        await lines.getByRole("listitem").allTextContents(),
        providers.attached.lyric.lines.map((line) => line.text),
      ),
    ).toBe(true);
    await verifyCredits(lyrics, providers.attached.lyric.attribution);
    await expect(
      lyrics.getByRole("heading", {
        name: providers.attached.lyric.rendition_label,
        exact: true,
      }),
    ).toBeVisible();
    await expect(
      lyrics.locator("img,script,iframe,audio,video,input,progress"),
    ).toHaveCount(0);
    expect(await page.evaluate(() => !("ownedChoicePayload" in window))).toBe(
      true,
    );
    checks.attachedExactStaticText = true;

    checkpoint = 9;
    await expect(input).toHaveValue(draft);
    expect(
      await input.evaluate((node, original) => node === original, editor),
    ).toBe(true);
    expect(
      (await page.evaluate(() => performance.timeOrigin)) === document,
    ).toBe(true);
    checks.unsavedEditorDomPreserved = true;
    expect(
      isDeepStrictEqual(await lyricState(page, ids, local.playlist), before) &&
        isDeepStrictEqual(
          await choicePeerState(peer, peerPlaylist),
          peerBefore,
        ) &&
        isDeepStrictEqual(await lyricOriginals(), originals),
    ).toBe(true);
    checks.privatePeerOriginalsPreserved = true;
    expect(
      isDeepStrictEqual(providers.primary.calls, providerBefore.primary) &&
        isDeepStrictEqual(providers.attached.calls, {
          ...providerBefore.attached,
          total: providerBefore.attached.total + 2,
          descriptor: 1,
          lyrics: 1,
        }) &&
        isDeepStrictEqual(providers.attached.lyricRequests, [
          { recording: providers.attached.reference },
        ]) &&
        providers.primary.lyricRequests.length === 0,
    ).toBe(true);
    expect(
      await page
        .locator("audio[data-hito-player]")
        .evaluate(
          (audio: HTMLAudioElement) =>
            audio.paused && audio.currentTime === 0 && !audio.currentSrc,
        ),
    ).toBe(true);
    checks.explicitOnlyNoPlayback = true;
    checkpoint = 10;
    browserEvidence = await evidence.finish();
    checks.safeCorrelatedWorkflows = true;
  } catch {
    failed = true;
  } finally {
    try {
      privateValues.push(
        ...(await context.cookies()).map((cookie) => cookie.value),
      );
      await page.goto("about:blank", { timeout: 5000 });
      await context.close();
      await peer.dispose();
      if (providers)
        await lyricDeadline(
          Promise.all([providers.primary.close(), providers.attached.close()]),
        );
    } catch {
      failed = true;
    }
  }
  const summary = {
    workflow: "provider-choice",
    browser: browserName,
    outcome: failed ? "failed" : "passed",
    checkpoint,
    checks,
    browserEvidence,
    counts: evidence?.counts ?? null,
    providers: providers
      ? { primary: providers.primary.calls, attached: providers.attached.calls }
      : null,
  };
  await writeFile("/evidence/canaries.json", JSON.stringify(privateValues), {
    mode: 0o600,
  });
  await writeFile("/evidence/summary.json", JSON.stringify(summary), {
    mode: 0o600,
  });
  if (failed)
    throw new Error(
      `Packaged lyric choice failed at checkpoint ${checkpoint}; counts=${JSON.stringify(summary)}`,
    );
  console.info(
    `Packaged lyric choice passed; counts=${JSON.stringify(summary)}`,
  );
});
