import { randomUUID } from "node:crypto";
import { writeFile } from "node:fs/promises";
import { isDeepStrictEqual } from "node:util";
import { expect, test } from "@playwright/test";
import type { Schema } from "../src/shared/schema";
import { artistReviewReader } from "./fixtures/artist-review-state";
import { verifyCredits } from "./fixtures/recording-attribution-state";
import { recordingLyricsProvider } from "./fixtures/recording-lyrics-provider";
import { prepareLyricsLibrary } from "./fixtures/recording-lyrics-workflow";
import { recordingLyricsEvidence } from "./fixtures/recording-lyrics-evidence";
import { recordingLyricsLifecycle } from "./fixtures/recording-lyrics-lifecycle";
import {
  assertLyricReply,
  lyricDeadline,
  lyricOriginals,
  lyricState,
} from "./fixtures/recording-lyrics-state";

type BrowserEvidence = Awaited<
  ReturnType<Awaited<ReturnType<typeof recordingLyricsEvidence>>["finish"]>
>;
test.use({
  trace: "off",
  screenshot: "off",
  video: "off",
  launchOptions: { args: [] },
});

test(
  "lifecycle: held lyric bodies retire before release and explicit retry survives navigation",
  recordingLyricsLifecycle,
);
test.skip(
  process.env.HITO_RECORDING_LYRICS_E2E !== "1",
  "Requires an explicitly disposable packaged lyric server",
);

test("reading: explicit lookup renders exact static lyrics and a negative retry without playback", async ({
  page,
  context,
  baseURL,
  browserName,
}) => {
  test.setTimeout(120_000);
  page.setDefaultTimeout(10_000);
  page.setDefaultNavigationTimeout(15_000);
  const query = `owned-lyric-query-${randomUUID()}`;
  const username = `lyric-owner-${randomUUID().slice(0, 8)}`;
  const password = `owned-lyric-password-${randomUUID()}`;
  const draft = `owned-unsaved-lyric-title-${randomUUID()}`;
  const privateValues = [
    query,
    username,
    password,
    draft,
    "/music/",
    "Fixture 440 Hz",
    "Fixture 660 Hz",
    "Hito test fixtures",
  ];
  const checks: Record<string, boolean> = {};
  let stage = 1;
  let failed = false;
  let provider: Awaited<ReturnType<typeof recordingLyricsProvider>> | null =
    null;
  let evidence: Awaited<ReturnType<typeof recordingLyricsEvidence>> | null =
    null;
  let browserEvidence: BrowserEvidence | null = null;
  let privateTargets: { ids: string[]; playlist: string } | null = null;
  try {
    expect(
      baseURL === "http://localhost:8080" &&
        process.env.PLAYWRIGHT_NO_COPY_PROMPT === "1",
    ).toBe(true);
    const read = artistReviewReader(page);
    expect(
      (await read<Schema<"SetupStatus">>("/api/v1/setup")).needs_setup,
    ).toBe(true);
    evidence = await recordingLyricsEvidence(context, baseURL!, privateValues);
    provider = await recordingLyricsProvider(query);
    const owned = provider;
    privateValues.push(...owned.privateValues);
    const originals = await lyricOriginals();
    await page.goto("/");
    await page.getByLabel("Username", { exact: true }).fill(username);
    await page.getByLabel("Password", { exact: true }).fill(password);
    await page
      .getByRole("button", { name: "Create administrator", exact: true })
      .click();
    await expect(
      page.getByRole("heading", { name: "Your library", exact: true }),
    ).toBeVisible();
    const user = await read<Schema<"User">>("/api/v1/me");
    expect(user.admin && user.username === username).toBe(true);
    privateValues.push(
      ...(await context.cookies()).map((cookie) => cookie.value),
    );

    stage = 2;
    const {
      local,
      connection,
      recording,
      observation,
      document,
      ids,
      before,
      providerBefore,
      lyrics,
    } = await prepareLyricsLibrary(page, owned, query, privateValues);
    privateTargets = { ids, playlist: local.playlist };
    checks.explicitOnly = true;
    evidence.arm();

    stage = 3;
    const input = page.getByRole("textbox", {
      name: "Track title",
      exact: true,
    });
    await input.fill(draft);
    const positive = await assertLyricReply(
      page,
      recording.id,
      observation.id,
      connection.revision,
      () =>
        lyrics
          .getByRole("button", { name: "Show lyrics", exact: true })
          .click(),
    );
    expect(
      isDeepStrictEqual(positive, {
        status: "ready",
        recording_id: recording.id,
        observation_id: observation.id,
        document: {
          recording: {
            id: owned.recordings[1].id,
            revision: owned.recordings[1].revision,
          },
          lyrics: owned.lyric,
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
        owned.lyric.lines.map((line) => line.text),
      ),
    ).toBe(true);
    await verifyCredits(lyrics, owned.lyric.attribution);
    await expect(
      lyrics.getByRole("heading", {
        name: owned.lyric.rendition_label,
        exact: true,
      }),
    ).toBeVisible();
    await expect(input).toHaveValue(draft);
    expect(
      (await page.evaluate(() => performance.timeOrigin)) === document &&
        (await page.evaluate(() => !("ownedLyricPayload" in window))),
    ).toBe(true);
    await expect(
      lyrics.locator("img,script,iframe,audio,video,input,progress"),
    ).toHaveCount(0);
    checks.staticExactText = true;
    checks.unsavedEditorPreserved = true;

    stage = 4;
    owned.setMode("null");
    const negative = await assertLyricReply(
      page,
      recording.id,
      observation.id,
      connection.revision,
      () =>
        lyrics
          .getByRole("button", { name: "Retry lyrics", exact: true })
          .click(),
    );
    expect(
      isDeepStrictEqual(negative, {
        status: "ready",
        recording_id: recording.id,
        observation_id: observation.id,
        document: {
          recording: {
            id: owned.recordings[1].id,
            revision: owned.recordings[1].revision,
          },
          lyrics: null,
        },
      }),
    ).toBe(true);
    await expect(
      lyrics.getByText("No lyrics found for this recording.", { exact: true }),
    ).toBeVisible();
    await expect(lines).toHaveCount(0);
    await expect(input).toHaveValue(draft);
    expect(
      isDeepStrictEqual(
        owned.lyricRequests,
        Array.from({ length: 2 }, () => ({
          recording: {
            id: owned.recordings[1].id,
            revision: owned.recordings[1].revision,
          },
        })),
      ),
    ).toBe(true);
    checks.nullResult = true;

    stage = 5;
    expect(
      isDeepStrictEqual(await lyricState(page, ids, local.playlist), before) &&
        isDeepStrictEqual(await lyricOriginals(), originals) &&
        isDeepStrictEqual(owned.calls, {
          ...providerBefore,
          total: providerBefore.total + 4,
          descriptor: 2,
          lyrics: 2,
        }),
    ).toBe(true);
    expect(
      await page
        .locator("audio[data-hito-player]")
        .evaluate(
          (audio: HTMLAudioElement) =>
            audio.paused && audio.currentTime === 0 && !audio.currentSrc,
        ),
    ).toBe(true);
    checks.statePreserved = true;
    checks.originalsPreserved = true;
    checks.noPlayback = true;
    browserEvidence = await evidence.finish(["ready", "not_found"]);
    expect(browserEvidence.counts.lyrics === 2).toBe(true);
    checks.safeWorkflows = true;
  } catch {
    failed = true;
  } finally {
    try {
      privateValues.push(
        ...(await context.cookies()).map((cookie) => cookie.value),
      );
      await page.goto("about:blank", { timeout: 5000 });
      await context.close();
      if (provider) await lyricDeadline(provider.close());
      if (evidence)
        browserEvidence = await evidence.finish(["ready", "not_found"]);
    } catch {
      failed = true;
    }
  }
  const summary = {
    workflow: "reading",
    browser: browserName,
    outcome: failed ? "failed" : "passed",
    stage,
    checks,
    browserEvidence,
    observed: evidence
      ? { counts: evidence.counts, failures: evidence.failures }
      : null,
    provider: provider?.calls ?? null,
  };
  await writeFile("/evidence/canaries.json", JSON.stringify(privateValues));
  await writeFile(
    "/evidence/request-diagnostics.json",
    JSON.stringify({
      targets: privateTargets,
      failures: evidence?.privateFailures ?? [],
    }),
  );
  await writeFile("/evidence/summary.json", JSON.stringify(summary));
  if (failed)
    throw new Error(
      `Packaged lyric reading failed at stage ${stage}; counts=${JSON.stringify(summary)}`,
    );
  console.info(
    `Packaged lyric reading passed; counts=${JSON.stringify(summary)}`,
  );
});
