import { randomUUID } from "node:crypto";
import { writeFile } from "node:fs/promises";
import { isDeepStrictEqual } from "node:util";
import { expect, test, type BrowserContext, type Page } from "@playwright/test";
import { recordingLyricsProvider } from "./recording-lyrics-provider";
import { recordingLyricsEvidence } from "./recording-lyrics-evidence";
import { prepareLyricsLibrary } from "./recording-lyrics-workflow";
import {
  assertLyricReply,
  lyricDeadline,
  lyricOriginals,
  lyricState,
} from "./recording-lyrics-state";

type Provider = Awaited<ReturnType<typeof recordingLyricsProvider>>;

export async function recordingLyricsLifecycle({
  page,
  context,
  baseURL,
  browserName,
}: {
  page: Page;
  context: BrowserContext;
  baseURL: string | undefined;
  browserName: string;
}) {
  test.setTimeout(120_000);
  page.setDefaultTimeout(10_000);
  page.setDefaultNavigationTimeout(15_000);
  const query = `owned-lyric-lifecycle-query-${randomUUID()}`;
  const username = `lyric-owner-${randomUUID().slice(0, 8)}`;
  const password = `owned-lyric-password-${randomUUID()}`;
  const privateValues = [
    query,
    username,
    password,
    "/music/",
    "Fixture 440 Hz",
    "Fixture 660 Hz",
    "Hito test fixtures",
  ];
  const reasons = ["cancelled", "ready", "cancelled", "ready"];
  const checks: Record<string, boolean> = {};
  let stage = 1;
  let failed = false;
  let provider: Provider | null = null;
  let evidence: Awaited<ReturnType<typeof recordingLyricsEvidence>> | null =
    null;
  let browserEvidence: Awaited<
    ReturnType<Awaited<ReturnType<typeof recordingLyricsEvidence>>["finish"]>
  > | null = null;
  let atFailure: Provider["calls"] | null = null;
  let privateTargets: { ids: string[]; playlist: string } | null = null;
  try {
    expect(
      baseURL === "http://localhost:8080" &&
        process.env.PLAYWRIGHT_NO_COPY_PROMPT === "1",
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
    privateValues.push(
      ...(await context.cookies()).map((cookie) => cookie.value),
    );
    stage = 2;
    const {
      local,
      recording,
      connection,
      observation,
      ids,
      before,
      providerBefore,
      lyrics,
    } = await prepareLyricsLibrary(page, owned, query, privateValues);
    privateTargets = { ids, playlist: local.playlist };
    evidence.arm();

    stage = 3;
    owned.setMode("held");
    const firstRequest = page.waitForRequest(
      (request) =>
        request.method() === "POST" &&
        new URL(request.url()).pathname ===
          `/api/v1/recordings/${recording.id}/lyrics`,
    );
    await lyrics
      .getByRole("button", { name: "Show lyrics", exact: true })
      .click();
    evidence.expectCancellation(await firstRequest);
    await expect.poll(() => owned.calls.held, { timeout: 4000 }).toBe(1);
    expect(owned.calls.dropped === 0 && owned.calls.released === 0).toBe(true);
    await expect(
      lyrics.getByText("Loading lyrics…", { exact: true }),
    ).toBeVisible();
    await lyrics
      .getByRole("button", { name: "Cancel lyric request", exact: true })
      .click();
    await expect(
      lyrics.getByRole("button", { name: "Show lyrics", exact: true }),
    ).toBeVisible();

    stage = 4;
    await expect
      .poll(() => owned.calls.dropped, {
        timeout: 1500,
        intervals: [25, 50, 100],
      })
      .toBe(1);
    expect(owned.calls.released === 0).toBe(true);
    checks.actualCancelRetirement = true;
    owned.setMode("ready");
    const first = await assertLyricReply(
      page,
      recording.id,
      observation.id,
      connection.revision,
      () =>
        lyrics
          .getByRole("button", { name: "Show lyrics", exact: true })
          .click(),
    );
    expect(first.status === "ready" && first.document.lyrics !== null).toBe(
      true,
    );
    await expect(
      lyrics.getByRole("list", { name: "Lyric lines", exact: true }),
    ).toBeVisible();
    checks.immediateRetry = true;

    stage = 5;
    owned.setMode("held");
    const nextRequest = page.waitForRequest(
      (request) =>
        request.method() === "POST" &&
        new URL(request.url()).pathname ===
          `/api/v1/recordings/${recording.id}/lyrics`,
    );
    await lyrics
      .getByRole("button", { name: "Retry lyrics", exact: true })
      .click();
    evidence.expectCancellation(await nextRequest);
    await expect.poll(() => owned.calls.held, { timeout: 4000 }).toBe(2);
    const document = await page.evaluate(() => performance.timeOrigin);
    await page
      .getByRole("link", { name: "Back to library", exact: true })
      .click();
    await expect(
      page.getByRole("heading", { name: "Your library", exact: true }),
    ).toBeVisible();
    expect(
      (await page.evaluate(() => performance.timeOrigin)) === document,
    ).toBe(true);
    await expect
      .poll(() => owned.calls.dropped, {
        timeout: 1500,
        intervals: [25, 50, 100],
      })
      .toBe(2);
    expect(owned.calls.released === 0).toBe(true);
    checks.navigationRetirement = true;

    stage = 6;
    owned.setMode("ready");
    await page.locator(`a[href="/recordings/${recording.id}"]`).first().click();
    await expect(
      lyrics.getByRole("button", { name: "Show lyrics", exact: true }),
    ).toBeVisible();
    expect(
      (await page.evaluate(() => performance.timeOrigin)) === document,
    ).toBe(true);
    const last = await assertLyricReply(
      page,
      recording.id,
      observation.id,
      connection.revision,
      () =>
        lyrics
          .getByRole("button", { name: "Show lyrics", exact: true })
          .click(),
    );
    expect(isDeepStrictEqual(last, first)).toBe(true);
    checks.freshNavigationRetry = true;
    expect(
      isDeepStrictEqual(await lyricState(page, ids, local.playlist), before) &&
        isDeepStrictEqual(await lyricOriginals(), originals),
    ).toBe(true);
    expect(
      isDeepStrictEqual(owned.calls, {
        ...providerBefore,
        total: providerBefore.total + 8,
        descriptor: 4,
        lyrics: 4,
        held: 2,
        dropped: 2,
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
    checks.preservedState = true;
    browserEvidence = await evidence.finish(reasons);
    expect(
      browserEvidence.counts.lyrics === 4 &&
        browserEvidence.counts.cancelledLyrics === 2,
    ).toBe(true);
    checks.safeWorkflows = true;
  } catch {
    failed = true;
    if (provider) atFailure = { ...provider.calls };
  } finally {
    try {
      privateValues.push(
        ...(await context.cookies()).map((cookie) => cookie.value),
      );
      await page.goto("about:blank", { timeout: 5000 });
      await context.close();
      if (provider) await lyricDeadline(provider.close());
      if (evidence) browserEvidence = await evidence.finish(reasons);
    } catch {
      failed = true;
    }
  }
  const summary = {
    workflow: "lifecycle",
    browser: browserName,
    outcome: failed ? "failed" : "passed",
    stage,
    checks,
    browserEvidence,
    observed: evidence
      ? { counts: evidence.counts, failures: evidence.failures }
      : null,
    atFailure,
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
      `Packaged lyric lifecycle failed at stage ${stage}; counts=${JSON.stringify(summary)}`,
    );
  console.info(
    `Packaged lyric lifecycle passed; counts=${JSON.stringify(summary)}`,
  );
}
