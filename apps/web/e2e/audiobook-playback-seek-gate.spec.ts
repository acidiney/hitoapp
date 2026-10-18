import { expect, test } from "@playwright/test";
import { auditAudiobookAudio } from "./fixtures/audiobook-playback-audio";
import { audiobookPlaybackEvidence } from "./fixtures/audiobook-playback-evidence";
import { audiobookPlaybackGateFixture } from "./fixtures/audiobook-playback-gates";
import { audiobookMediaState } from "./fixtures/audiobook-playback-state";

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

test("native seek cancellation pairs only with complete source and trusted retained media evidence", async ({
  context,
  page,
  browserName,
}) => {
  test.skip(
    browserName !== "chromium",
    "Characterizes the observed Chromium native range retirement",
  );
  test.setTimeout(30_000);
  page.setDefaultTimeout(5000);
  const fixture = await audiobookPlaybackGateFixture((length, request) =>
    request.ordinal === 2 && request.range === "bytes=0-"
      ? {
          status: 206,
          contentRange: `bytes 0-${length - 1}/${length}`,
          prefixBytes: 1_048_576,
        }
      : undefined,
  );
  const evidence = await audiobookPlaybackEvidence(
    context,
    fixture.origin,
    browserName,
    [],
  );
  const diagnostics = {
    stage: 0,
    failed: 0,
    baseline: 0,
    mutations: 0,
    rejected: 0,
    wrong_raw: 0,
    mutation_failures: 0,
    raw_failure_mask: 0,
    native_metadata: -1,
    native_seeking: -1,
    native_seeked: -1,
    native_trusted: -1,
    native_ready: -1,
    native_network: -1,
    native_error: -1,
    native_paused: -1,
    native_position: -1,
    native_duration: -1,
    native_rate: -1,
    observed_until: -1,
  };
  let rows: number[][] = [];
  let baseline: ReturnType<typeof auditAudiobookAudio> | undefined;
  let activeCheckpoint:
    | Awaited<ReturnType<typeof evidence.seekCheckpoint>>
    | undefined;
  let checkpointFinished = false;
  try {
    diagnostics.stage = 1;
    await page.goto(fixture.origin);
    await expect.poll(() => evidence.pending()).toBe(0);
    await evidence.arm(fixture.target);
    const path = fixture.target.audio[0].path;
    const play = page.getByRole("button", {
      name: "Play owned audio",
      exact: true,
    });
    await play.click();
    await expect
      .poll(async () => {
        const media = await audiobookMediaState(page, path);
        return (
          media.exact &&
          media.playing &&
          media.position > 0.2 &&
          media.position < 3
        );
      })
      .toBe(true);
    await expect.poll(() => evidence.pending()).toBe(0);
    expect(evidence.audio().verified).toBe(1);
    expect(evidence.audio().violations).toBe(0);
    diagnostics.stage = 2;
    await page.locator("audio[data-hito-player]").evaluate(async (element) => {
      const audio = element as HTMLAudioElement;
      await new Promise<void>((resolve, reject) => {
        const timer = setTimeout(
          () => reject(new Error("Owned media clear deadline")),
          3000,
        );
        audio.addEventListener(
          "emptied",
          () => {
            clearTimeout(timer);
            resolve();
          },
          { once: true },
        );
        audio.pause();
        audio.removeAttribute("src");
        audio.load();
      });
      const native = { metadata: 0, seeking: 0, seeked: 0, trusted: 0 };
      (globalThis as unknown as Record<string, unknown>).__hitoSeekGate =
        native;
      for (const [index, kind] of ["seeking", "seeked"].entries())
        audio.addEventListener(kind, (event) => {
          const key = kind as "seeking" | "seeked";
          native[key] = Math.min(2, native[key] + 1);
          if (event.isTrusted) native.trusted |= 1 << (index + 1);
        });
      audio.addEventListener(
        "loadedmetadata",
        (event) => {
          native.metadata = Math.min(2, native.metadata + 1);
          if (event.isTrusted) native.trusted |= 1;
        },
        { once: true },
      );
      audio.addEventListener(
        "canplay",
        (event) => {
          if (event.isTrusted) audio.currentTime = 84.125;
        },
        { once: true },
      );
    });
    const checkpoint = await evidence.seekCheckpoint(page, path, 84_125);
    activeCheckpoint = checkpoint;
    diagnostics.stage = 3;
    await play.click();
    await expect
      .poll(async () => {
        const media = await audiobookMediaState(page, path);
        return (
          media.exact &&
          media.playing &&
          media.position > 84.2 &&
          media.position < 87
        );
      })
      .toBe(true);
    await expect.poll(() => evidence.pending()).toBe(0);
    await expect.poll(() => checkpoint.ready()).toBe(true);
    await checkpoint.finish();
    checkpointFinished = true;
    diagnostics.stage = 4;
    const actual = evidence.audioEvidence();
    baseline = auditAudiobookAudio(
      actual.records,
      actual.sources,
      browserName,
      actual.witnesses,
    );
    diagnostics.baseline = Number(
      baseline.violations === 1 &&
        baseline.explained_seek_cancellations === 1 &&
        baseline.unexplained_violations === 0 &&
        baseline.body_failures === 1 &&
        baseline.verified === 2 &&
        baseline.requests === 3 &&
        evidence.counts.workflow_failures === 1,
    );
    const clone = () => ({
      records: actual.records.map((record) => ({
        ...record,
        diagnostic: { ...record.diagnostic },
      })),
      sources: actual.sources,
      witnesses: actual.witnesses.map((witness) => ({
        ...witness,
        events: witness.events.map((event) => ({ ...event })),
      })),
      browser: browserName as string,
    });
    type Input = ReturnType<typeof clone>;
    const cancelled = (input: Input) =>
      input.records.find((record) => record.failed)!;
    const replacement = (input: Input) =>
      input.records.find((record) =>
        /^bytes=[1-9]\d*-$/.test(record.requestRange ?? ""),
      )!;
    const mutations: { change(input: Input): void; raw: number }[] = [
      {
        change: (input) => {
          cancelled(input).status = 200;
        },
        raw: 1,
      },
      {
        change: (input) => {
          cancelled(input).range = undefined;
        },
        raw: 1,
      },
      {
        change: (input) => {
          cancelled(input).contentLength = "1";
        },
        raw: 1,
      },
      {
        change: (input) => {
          cancelled(input).requestRange = "bytes=1-";
        },
        raw: 1,
      },
      {
        change: (input) => {
          const peer = replacement(input);
          const source = input.sources.find(
            (source) => source.url === peer.url,
          )!;
          const start = 2_800_000;
          peer.requestRange = `bytes=${start}-`;
          peer.range = `bytes ${start}-${source.bytes.length - 1}/${source.bytes.length}`;
          peer.bytes = source.bytes.subarray(start);
          peer.bodyBytes = peer.bytes.length;
          peer.contentLength = String(peer.bytes.length);
        },
        raw: 1,
      },
      {
        change: (input) => {
          input.witnesses[0].events[2].owner++;
        },
        raw: 1,
      },
      {
        change: (input) => {
          input.witnesses[0].changed = true;
        },
        raw: 1,
      },
      {
        change: (input) => {
          cancelled(input).diagnostic.network = 9;
        },
        raw: 1,
      },
      {
        change: (input) => {
          cancelled(input).diagnostic.body = 9;
        },
        raw: 1,
      },
      {
        change: (input) => {
          input.browser = "firefox";
        },
        raw: 1,
      },
      {
        change: (input) => {
          const peer = replacement(input);
          peer.finished = 0;
          peer.bytes = undefined;
          peer.bodyBytes = null;
        },
        raw: 2,
      },
      {
        change: (input) => {
          const start = cancelled(input).started;
          input.records = input.records.filter(
            (record) => record.started >= start,
          );
        },
        raw: 1,
      },
      {
        change: (input) => {
          input.witnesses[0].events[4].connected = false;
        },
        raw: 1,
      },
      {
        change: (input) => {
          input.witnesses[0].events[2].trusted = false;
        },
        raw: 1,
      },
      {
        change: (input) => {
          input.witnesses = [];
        },
        raw: 1,
      },
      {
        change: (input) => {
          input.witnesses[0].events[1].unique = false;
        },
        raw: 1,
      },
      {
        change: (input) => {
          input.records.push({ ...replacement(input) });
        },
        raw: 1,
      },
      {
        change: (input) => {
          input.records.push({ ...cancelled(input) });
        },
        raw: 2,
      },
      {
        change: (input) => {
          cancelled(input).diagnostic.time = -1;
        },
        raw: 1,
      },
      {
        change: (input) => {
          cancelled(input).diagnostic.failureReceipt = -1;
        },
        raw: 1,
      },
      {
        change: (input) => {
          cancelled(input).diagnostic.end = -1;
        },
        raw: 1,
      },
      {
        change: (input) => {
          input.witnesses[0].events[2].source = input.sources[1].url;
        },
        raw: 1,
      },
      {
        change: (input) => {
          const prior = input.records.find(
            (record) => record.started < cancelled(input).started,
          )!;
          prior.verifiedAt = cancelled(input).started + 1;
        },
        raw: 1,
      },
      {
        change: (input) => {
          const witness = input.witnesses[0];
          witness.observedUntil = witness.events[4].time - 1;
        },
        raw: 1,
      },
      {
        change: (input) => {
          const witness = input.witnesses[0];
          witness.observedUntil = witness.events[0].time + 10_001;
        },
        raw: 1,
      },
      {
        change: (input) => {
          replacement(input).diagnostic.finishReceipt =
            input.witnesses[0].observedUntil + 6;
        },
        raw: 1,
      },
    ];
    diagnostics.stage = 5;
    for (const [index, mutation] of mutations.entries()) {
      const input = clone();
      mutation.change(input);
      const audit = auditAudiobookAudio(
        input.records,
        input.sources,
        input.browser,
        input.witnesses,
      );
      diagnostics.mutations++;
      if (
        audit.explained_seek_cancellations === 0 &&
        audit.unexplained_violations === mutation.raw
      )
        diagnostics.rejected++;
      else diagnostics.mutation_failures |= 1 << index;
      if (audit.violations !== mutation.raw) {
        diagnostics.wrong_raw++;
        diagnostics.raw_failure_mask |= 1 << index;
      }
    }
    const first = actual.witnesses[0].events[0].time;
    rows = actual.witnesses[0].events.map((event) => [
      event.kind,
      event.owner,
      Math.round(event.time - first),
      event.position,
      Number(event.trusted),
      Number(event.connected),
      Number(event.active),
      Number(event.unique),
    ]);
  } catch {
    diagnostics.failed++;
  } finally {
    try {
      const native = await bounded(
        page.evaluate(() => {
          const audio = document.querySelector<HTMLAudioElement>(
            "audio[data-hito-player]",
          );
          const counters = (globalThis as unknown as Record<string, unknown>)
            .__hitoSeekGate as
            | {
                metadata: number;
                seeking: number;
                seeked: number;
                trusted: number;
              }
            | undefined;
          const safe = (value: number | undefined, limit: number) =>
            typeof value === "number" &&
            Number.isFinite(value) &&
            value >= 0 &&
            value <= limit
              ? Math.round(value)
              : -1;
          return {
            native_metadata: counters?.metadata ?? -1,
            native_seeking: counters?.seeking ?? -1,
            native_seeked: counters?.seeked ?? -1,
            native_trusted: counters?.trusted ?? -1,
            native_ready: audio?.readyState ?? -1,
            native_network: audio?.networkState ?? -1,
            native_error: audio?.error?.code ?? 0,
            native_paused: Number(audio?.paused ?? false),
            native_position: safe(
              audio ? audio.currentTime * 1000 : undefined,
              90_000,
            ),
            native_duration: safe(
              audio ? audio.duration * 1000 : undefined,
              90_100,
            ),
            native_rate: safe(
              audio ? audio.playbackRate * 1000 : undefined,
              4000,
            ),
          };
        }),
      );
      Object.assign(diagnostics, native);
      if (activeCheckpoint && !checkpointFinished) {
        await bounded(activeCheckpoint.finish());
        checkpointFinished = true;
      }
      const witnesses = evidence.audioEvidence().witnesses;
      if (witnesses.length) {
        const first = witnesses[0].events[0]?.time ?? 0;
        const observedUntil = witnesses[0].observedUntil - first;
        diagnostics.observed_until =
          first > 0 &&
          Number.isFinite(observedUntil) &&
          observedUntil >= 0 &&
          observedUntil <= 180_000
            ? Math.round(observedUntil)
            : -1;
        rows = witnesses[0].events.map((event) => [
          event.kind,
          event.owner,
          Math.round(event.time - first),
          event.position,
          Number(event.trusted),
          Number(event.connected),
          Number(event.active),
          Number(event.unique),
        ]);
      }
    } catch {
      diagnostics.failed++;
    }
    try {
      await context.close();
    } catch {
      diagnostics.failed++;
    }
    try {
      await evidence.finish();
    } catch {
      diagnostics.failed++;
    }
    try {
      await fixture.close();
    } catch {
      diagnostics.failed++;
    }
  }
  const passed =
    diagnostics.failed + diagnostics.wrong_raw === 0 &&
    diagnostics.baseline === 1 &&
    diagnostics.mutations === 26 &&
    diagnostics.rejected === diagnostics.mutations &&
    fixture.counts.errors +
      evidence.counts.invalid +
      evidence.counts.forbidden ===
      0;
  const report = JSON.stringify({
    passed,
    ...diagnostics,
    baseline,
    witness: rows,
    requests: evidence.diagnostics().requests,
    server: fixture.counts,
    transport: evidence.counts,
  });
  if (Buffer.byteLength(report) > 4096)
    throw new Error("Owned seek gate diagnostic bound");
  console.log(`audiobook_playback_seek_gate ${report}`);
  if (!passed)
    throw new Error("The owned native seek cancellation gate failed");
});

async function bounded<T>(work: Promise<T>) {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([
      work,
      new Promise<never>((_, reject) => {
        timer = setTimeout(
          () => reject(new Error("Owned seek diagnostic deadline")),
          2000,
        );
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}
