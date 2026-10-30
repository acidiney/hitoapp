import { randomUUID } from "node:crypto";
import { expect, test, type BrowserContext, type Page } from "@playwright/test";
import type { Schema } from "../src/shared/schema";
import { audiobookPlaybackEvidence } from "./fixtures/audiobook-playback-evidence";
import {
  audiobookPlaybackGateFixture,
  type GateAudioReply,
} from "./fixtures/audiobook-playback-gates";
import {
  audiobookMediaState,
  audiobookNavigationCheck,
  audiobookPlaybackPrivateValues,
  audiobookPlaybackUsername,
} from "./fixtures/audiobook-playback-state";

type Fixture = Awaited<ReturnType<typeof audiobookPlaybackGateFixture>>;
type Evidence = Awaited<ReturnType<typeof audiobookPlaybackEvidence>>;
type Browser = { context: BrowserContext; page: Page; browserName: string };
test.use({
  trace: "off",
  screenshot: "off",
  video: "off",
  launchOptions: { args: [] },
});
test.skip(
  process.env.HITO_AUDIOBOOK_PLAYBACK_GATES_E2E !== "1",
  "Requires explicit owned audiobook playback gate opt-in",
);

test.describe("audiobook playback gate regressions", () => {
  test("retrospective console inspection rejects the actual setup username", async ({
    context,
    page,
    browserName,
  }) => {
    await runGate(
      { context, page, browserName },
      "setup-username",
      async ({ page, values, evidence }) => {
        await page.evaluate(
          (username) => console.info({ username }),
          audiobookPlaybackUsername,
        );
        await expect
          .poll(() => evidence.counts.console_bytes)
          .toBeGreaterThan(0);
        values.push(...audiobookPlaybackPrivateValues("Owned gate password"));
        return {};
      },
      { arm: false, check: (evidence) => evidence.counts.private_console > 0 },
    );
  });

  for (const framing of [
    "partial-status-full-body",
    "full-status-sliced-body",
    "contradicting-request-range",
  ] as const) {
    test(`rejects ${framing} through the actual audio response audit`, async ({
      context,
      page,
      browserName,
    }) => {
      const reply = (length: number): GateAudioReply =>
        framing === "partial-status-full-body"
          ? { status: 206 }
          : framing === "full-status-sliced-body"
            ? {
                status: 200,
                start: 0,
                end: 15,
                contentRange: `bytes 0-15/${length}`,
              }
            : {
                status: 206,
                start: 16,
                end: 31,
                contentRange: `bytes 16-31/${length}`,
              };
      await runGate(
        { context, page, browserName },
        framing,
        async ({ page, fixture, evidence }) => {
          const status = await page.evaluate(
            async ({ path, ranged }) => {
              const response = await fetch(path, {
                headers: ranged ? { Range: "bytes=0-15" } : {},
              });
              const body = await response.arrayBuffer();
              return body.byteLength <= 4_194_304 ? response.status : 0;
            },
            {
              path: fixture.target.audio[0].path,
              ranged: framing !== "partial-status-full-body",
            },
          );
          await expect.poll(() => evidence.pending()).toBe(0);
          return { delivered_status: status };
        },
        {
          reply,
          check: (evidence) =>
            evidence.audio().violations > 0 && evidence.audio().verified === 0,
        },
      );
    });
  }

  for (const interruption of [
    "replacement",
    "pause-restart",
    "load-restart",
  ] as const) {
    test(`navigation rejects resumed ${interruption} despite matching source and advancing position`, async ({
      context,
      page,
      browserName,
    }) => {
      let rejected = 0;
      await runGate(
        { context, page, browserName },
        interruption,
        async ({ page, fixture }) => {
          const path = fixture.target.audio[0].path;
          await startAudio(page, path);
          const first = await audiobookMediaState(page, path);
          const checkpoint = await audiobookNavigationCheck(
            page,
            path,
            await page.evaluate(() => performance.timeOrigin),
            first.position,
          );
          await page.evaluate(async (kind) => {
            let audio = document.querySelector<HTMLAudioElement>(
              "audio[data-hito-player]",
            )!;
            const position = audio.currentTime;
            audio.pause();
            if (kind === "replacement") {
              const replacement = audio.cloneNode() as HTMLAudioElement;
              audio.replaceWith(replacement);
              audio = replacement;
            }
            if (kind !== "pause-restart") {
              audio.load();
              await new Promise<void>((resolve, reject) => {
                const timeout = setTimeout(
                  () => reject(new Error("Owned media metadata timeout")),
                  5000,
                );
                audio.addEventListener(
                  "loadedmetadata",
                  () => {
                    clearTimeout(timeout);
                    resolve();
                  },
                  { once: true },
                );
              });
            }
            audio.currentTime = position + 0.25;
            await audio.play();
          }, interruption);
          await expect
            .poll(async () => {
              const state = await audiobookMediaState(page, path);
              return (
                state.exact && state.playing && state.position > first.position
              );
            })
            .toBe(true);
          rejected = Number(!(await checkpoint()));
          return { rejected };
        },
        { check: () => rejected === 1 },
      );
    });
  }

  for (const framing of ["complete", "open-range", "ignored-range"] as const) {
    test(`accepts legal ${framing} owned audio framing`, async ({
      context,
      page,
      browserName,
    }) => {
      await runGate(
        { context, page, browserName },
        `valid-${framing}`,
        async ({ page, fixture, evidence }) => {
          const status = await page.evaluate(
            async ({ path, ranged }) => {
              const response = await fetch(path, {
                headers: ranged ? { Range: "bytes=0-" } : {},
              });
              const bytes = await response.arrayBuffer();
              return bytes.byteLength <= 4_194_304 ? response.status : 0;
            },
            {
              path: fixture.target.audio[0].path,
              ranged: framing !== "complete",
            },
          );
          await expect.poll(() => evidence.pending()).toBe(0);
          return { delivered_status: status };
        },
        {
          reply:
            framing === "ignored-range" ? () => ({ status: 200 }) : undefined,
          check: (evidence) =>
            evidence.audio().violations === 0 &&
            evidence.audio().verified === 1 &&
            evidence.counts.invalid +
              evidence.counts.forbidden +
              evidence.counts.workflow_failures ===
              0,
        },
      );
    });
  }

  test("accepts uninterrupted playback of the same actual element", async ({
    context,
    page,
    browserName,
  }) => {
    let retained = 0;
    await runGate(
      { context, page, browserName },
      "valid-retained-media",
      async ({ page, fixture }) => {
        const path = fixture.target.audio[0].path;
        await startAudio(page, path);
        const first = await audiobookMediaState(page, path);
        const checkpoint = await audiobookNavigationCheck(
          page,
          path,
          await page.evaluate(() => performance.timeOrigin),
          first.position,
        );
        await expect
          .poll(
            async () =>
              (await audiobookMediaState(page, path)).position - first.position,
          )
          .toBeGreaterThan(0.25);
        retained = Number(await checkpoint());
        return { retained };
      },
      { check: () => retained === 1 },
    );
  });

  test("accepts paused seek progress and the actual outgoing media snapshot during handoff", async ({
    context,
    page,
    browserName,
  }) => {
    const operations: string[] = [];
    await runGate(
      { context, page, browserName },
      "valid-progress-handoff",
      async ({ page, fixture, evidence }) => {
        await startAudio(page, fixture.target.audio[0].path);
        await page.locator("audio[data-hito-player]").evaluate((element) => {
          const audio = element as HTMLAudioElement;
          audio.currentTime = 12.5;
          audio.pause();
        });
        await expect
          .poll(async () => {
            const media = await audiobookMediaState(
              page,
              fixture.target.audio[0].path,
            );
            return (
              media.exact &&
              !media.playing &&
              Math.abs(media.position - 12.5) < 0.01
            );
          })
          .toBe(true);
        const paused = batch(fixture.target.recordings[0], 12_500);
        operations.push(paused.operations[0].id);
        await submitProgress(page, fixture, paused);
        const outgoing = batch(fixture.target.recordings[0], 13_250);
        operations.push(outgoing.operations[0].id);
        const status = await page.evaluate(
          async ({ account, value, next }) => {
            const audio = document.querySelector<HTMLAudioElement>(
              "audio[data-hito-player]",
            )!;
            audio.currentTime = 13.25;
            const pending = fetch("/api/v1/sync", {
              method: "POST",
              headers: {
                "content-type": "application/json",
                "x-hito-request": "1",
                "x-hito-account": account,
              },
              body: JSON.stringify(value),
            });
            audio.src = next;
            await audio.play();
            const response = await pending;
            const bytes = await response.arrayBuffer();
            return bytes.byteLength <= 4096 ? response.status : 0;
          },
          {
            account: fixture.target.account,
            value: outgoing,
            next: fixture.target.audio[1].path,
          },
        );
        await expect.poll(() => evidence.pending()).toBe(0);
        return { handoff_status: status };
      },
      {
        check: (evidence) =>
          evidence.counts.invalid +
            evidence.counts.forbidden +
            evidence.counts.workflow_failures ===
            0 &&
          evidence.accepted().length === 2 &&
          evidence
            .accepted()
            .every((value) => operations.includes(value.operations[0].id)),
      },
    );
  });

  test("rejects an unrelated recording's invented progress before a later normal write", async ({
    context,
    page,
    browserName,
  }) => {
    let normal = "";
    await runGate(
      { context, page, browserName },
      "progress-media-correlation",
      async ({ page, fixture, evidence }) => {
        await startAudio(page, fixture.target.audio[0].path);
        const invented = batch(fixture.target.recordings[1], 90_000);
        await submitProgress(page, fixture, invented);
        await page.evaluate(async (path) => {
          const audio = document.querySelector<HTMLAudioElement>(
            "audio[data-hito-player]",
          )!;
          audio.pause();
          audio.src = path;
          await audio.play();
        }, fixture.target.audio[1].path);
        await expect
          .poll(async () => {
            const state = await audiobookMediaState(
              page,
              fixture.target.audio[1].path,
            );
            return (
              state.exact &&
              state.playing &&
              state.position > 0.2 &&
              state.position < 2
            );
          })
          .toBe(true);
        const position = await page
          .locator("audio[data-hito-player]")
          .evaluate((audio) =>
            Math.floor((audio as HTMLAudioElement).currentTime * 1000),
          );
        const valid = batch(fixture.target.recordings[1], position);
        normal = valid.operations[0].id;
        await submitProgress(page, fixture, valid);
        await expect.poll(() => evidence.pending()).toBe(0);
        return { normal_written: Number(normal !== "") };
      },
      {
        check: (evidence) => {
          const accepted = evidence.accepted();
          return (
            evidence.counts.forbidden + evidence.counts.invalid > 0 &&
            accepted.length === 1 &&
            accepted[0].operations[0].id === normal
          );
        },
      },
    );
  });
});

function batch(recording_id: string, position_ms: number): Schema<"SyncBatch"> {
  return {
    device_id: randomUUID(),
    operations: [
      {
        id: randomUUID(),
        action: { kind: "progress_set", recording_id, position_ms, speed: 1 },
      },
    ],
  };
}

async function startAudio(page: Page, path: string) {
  await page
    .getByRole("button", { name: "Play owned audio", exact: true })
    .click();
  await expect
    .poll(async () => {
      const state = await audiobookMediaState(page, path);
      return (
        state.exact &&
        state.playing &&
        state.position > 0.2 &&
        state.position < 3
      );
    })
    .toBe(true);
}

async function submitProgress(
  page: Page,
  fixture: Fixture,
  value: Schema<"SyncBatch">,
) {
  return page.evaluate(
    async ({ account, value }) => {
      try {
        const response = await fetch("/api/v1/sync", {
          method: "POST",
          headers: {
            "content-type": "application/json",
            "x-hito-request": "1",
            "x-hito-account": account,
          },
          body: JSON.stringify(value),
        });
        const body = await response.arrayBuffer();
        return body.byteLength <= 4096 ? response.status : 0;
      } catch {
        return 0;
      }
    },
    { account: fixture.target.account, value },
  );
}

async function runGate(
  browser: Browser,
  gate: string,
  exercise: (
    value: Browser & { fixture: Fixture; evidence: Evidence; values: string[] },
  ) => Promise<Record<string, number>>,
  options: {
    arm?: boolean;
    reply?: (length: number) => GateAudioReply;
    check: (evidence: Evidence) => boolean;
  },
) {
  const fixture = await audiobookPlaybackGateFixture(options.reply);
  const values: string[] = [];
  const evidence = await audiobookPlaybackEvidence(
    browser.context,
    fixture.origin,
    browser.browserName,
    values,
  );
  let failed = 0;
  let diagnostics: Record<string, number> = {};
  try {
    await browser.page.goto(fixture.origin);
    await expect.poll(() => evidence.pending()).toBe(0);
    if (options.arm !== false) await evidence.arm(fixture.target);
    diagnostics = await exercise({ ...browser, fixture, evidence, values });
  } catch {
    failed++;
  } finally {
    try {
      await browser.context.close();
    } catch {
      failed++;
    }
    try {
      await evidence.finish();
    } catch {
      failed++;
    }
    try {
      await fixture.close();
    } catch {
      failed++;
    }
  }
  const passed =
    options.check(evidence) && fixture.counts.errors === 0 && failed === 0;
  const report = JSON.stringify({
    gate,
    failed,
    passed,
    ...diagnostics,
    server: fixture.counts,
    evidence: evidence.counts,
    audio: evidence.audio(),
  });
  if (Buffer.byteLength(report) > 4096)
    throw new Error("Owned playback gate diagnostic bound");
  console.log(`audiobook_playback_gate ${report}`);
  if (!passed) throw new Error(`The owned playback ${gate} regression failed`);
}
