import { isDeepStrictEqual } from "node:util";
import {
  expect,
  type BrowserContext,
  type Page,
  type Request,
  type Response,
} from "@playwright/test";
import type { components } from "../../src/generated";
import { captureConsolePrivacy } from "./console-privacy";
import { audiobookProgressObservation } from "./audiobook-playback-progress";
import { audiobookPlaybackDiagnostics } from "./audiobook-playback-diagnostics";
import { audiobookMediaState } from "./audiobook-playback-state";
import {
  auditAudiobookAudio,
  verifiedAudiobookAudio,
  type AudiobookAudioEvidence,
} from "./audiobook-playback-audio";
import {
  audiobookSeekCheckpoint,
  type AudiobookSeekWitness,
} from "./audiobook-playback-seek";

type Schema = components["schemas"];
type Target = {
  account: string;
  book: Schema["AudiobookDetail"];
  recordings: string[];
  sources: Schema["Source"][][];
  audio: { path: string; bytes: Buffer }[];
};
type Kind =
  | "preparation"
  | "book"
  | "source"
  | "artwork"
  | "availability"
  | "navigation"
  | "progress"
  | "audio"
  | "forbidden";
type Started = { armed: boolean; kind: Kind; batch?: Schema["SyncBatch"] };
type Audio = AudiobookAudioEvidence;
const uuid = /^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/;

function refreshAudioTiming(request: Request, capture: Audio) {
  try {
    const timing = request.timing();
    const time = timing.startTime;
    const delay = capture.diagnostic.receipt - time;
    if (Number.isFinite(time) && time > 0 && delay >= -5 && delay <= 10_000)
      capture.diagnostic.time = time;
    if (
      capture.diagnostic.time > 0 &&
      Number.isFinite(timing.responseEnd) &&
      timing.responseEnd >= 0 &&
      timing.responseEnd <= 180_000
    )
      capture.diagnostic.end = capture.diagnostic.time + timing.responseEnd;
  } catch {}
}

function audioBodyDiagnostic(message: string) {
  const reasons = [
    [1, "Missing content of resource for given requestId"],
    [2, "No resource with given identifier found"],
    [3, "Request content was evicted from inspector cache"],
    [3, "Resource content was evicted from inspector cache"],
    [5, "No data found for resource with given identifier"],
  ] as const;
  for (const [code, reason] of reasons) {
    const exact = `Protocol error (Network.getResponseBody): ${reason}`;
    if (message === exact || message === `response.body: ${exact}`) return code;
  }
  return 9;
}

export async function audiobookPlaybackEvidence(
  context: BrowserContext,
  origin: string,
  browser: string,
  privateValues: string[],
) {
  const progress = await audiobookProgressObservation(context, origin);
  const timing = await audiobookPlaybackDiagnostics(context, origin);
  const counts = {
    preparation_requests: 0,
    preparation_failures: 0,
    preparation_status_errors: 0,
    preparation_body_failures: 0,
    workflow_requests: 0,
    workflow_failures: 0,
    forbidden: 0,
    invalid: 0,
    page_errors: 0,
    book_reads: 0,
    source_reads: 0,
    artwork_reads: 0,
    navigations: 0,
    progress_submitted: 0,
    progress_accepted: 0,
    audio_requests: 0,
    audio_bytes: 0,
    response_bytes: 0,
    console_bytes: 0,
    private_console: 0,
    malformed_console: 0,
    chapter_handoffs: 0,
    bookmark_handoffs: 0,
    playback_started: 0,
    playback_finished: 0,
    probe_text: 0,
    probe_snapshot: 0,
  };
  const pending = new Set<Promise<void>>();
  const active = new Set<Request>();
  const starts = new WeakMap<Request, Started>();
  const captures = new Map<Request, Audio>();
  const seekWitnesses: AudiobookSeekWitness[] = [];
  const seekHandles = new Set<
    Awaited<ReturnType<typeof audiobookSeekCheckpoint>>
  >();
  let seekArmed = false;
  const accepted: Schema["SyncBatch"][] = [];
  const operationIds = new Set<string>();
  const logs = new Map<string, string>();
  const texts: string[] = [];
  const watched = new WeakSet<Page>();
  let sequence = 0;
  let target: Target | null = null;
  const track = (work: Promise<void>) => {
    if (pending.size >= 64) counts.invalid++;
    const joined = work
      .catch(() => {
        counts.invalid++;
      })
      .finally(() => pending.delete(joined));
    pending.add(joined);
    return joined;
  };
  const watch = (page: Page) => {
    if (watched.has(page)) return;
    watched.add(page);
    page.on("pageerror", () => counts.page_errors++);
  };
  context.on("page", watch);
  context.pages().forEach(watch);

  function classify(request: Request): Started {
    const url = new URL(request.url());
    const path = url.pathname;
    const body = request.postData();
    const bad = { armed: target !== null, kind: "forbidden" } as const;
    if (
      url.origin !== origin ||
      /\/(?:plugins|community|downloads?|preparations|resolve)(?:\/|$)/.test(
        path,
      ) ||
      (body !== null && Buffer.byteLength(body) > 16_384)
    )
      return bad;
    if (!target)
      return {
        armed: false,
        kind: /\/audio(?:\/|$)/.test(path) ? "forbidden" : "preparation",
      };
    const headers = request.headers();
    const guarded =
      headers["x-hito-request"] === "1" &&
      headers["x-hito-account"] === target.account;
    if (
      request.method() === "POST" &&
      path === "/api/v1/sync" &&
      !url.search &&
      guarded &&
      headers.origin === origin &&
      body
    ) {
      const batch = JSON.parse(body) as Schema["SyncBatch"];
      const operation = batch.operations?.[0];
      const action = operation?.action;
      if (
        !uuid.test(batch.device_id) ||
        batch.operations?.length !== 1 ||
        !operation ||
        !uuid.test(operation.id) ||
        operationIds.has(operation.id) ||
        action?.kind !== "progress_set" ||
        !target.recordings.includes(action.recording_id) ||
        !Number.isSafeInteger(action.position_ms) ||
        action.position_ms < 0 ||
        action.position_ms > 90_000 ||
        action.speed !== 1
      )
        return bad;
      operationIds.add(operation.id);
      counts.progress_submitted++;
      return { armed: true, kind: "progress", batch };
    }
    if (request.method() !== "GET" || body !== null || url.search) return bad;
    if (target.audio.some((source) => source.path === path))
      return { armed: true, kind: "audio" };
    if (guarded && path === `/api/v1/audiobooks/${target.book.playlist.id}`) {
      counts.book_reads++;
      return { armed: true, kind: "book" };
    }
    if (
      guarded &&
      target.recordings.some(
        (id) => path === `/api/v1/recordings/${id}/sources`,
      )
    ) {
      counts.source_reads++;
      return { armed: true, kind: "source" };
    }
    if (
      target.recordings.some(
        (id) => path === `/api/v1/recordings/${id}/artwork`,
      )
    ) {
      counts.artwork_reads++;
      return { armed: true, kind: "artwork" };
    }
    if (
      guarded &&
      target.recordings.some(
        (id) => path === `/api/v1/recordings/${id}/artwork/availability`,
      )
    )
      return { armed: true, kind: "availability" };
    if (
      headers["x-inertia"] === "true" &&
      ["/audiobooks", `/audiobooks/${target.book.playlist.id}`].includes(path)
    ) {
      counts.navigations++;
      return { armed: true, kind: "navigation" };
    }
    return bad;
  }

  context.on("request", (request) => {
    const receipt = Date.now();
    try {
      if (target) counts.workflow_requests++;
      else counts.preparation_requests++;
      if (counts.preparation_requests + counts.workflow_requests > 300)
        counts.invalid++;
      const start = classify(request);
      starts.set(request, start);
      active.add(request);
      if (start.kind === "forbidden") counts.forbidden++;
      if (start.kind !== "audio") return;
      counts.audio_requests++;
      if (captures.size >= 16) {
        counts.invalid++;
        return;
      }
      captures.set(request, {
        url: request.url(),
        method: request.method(),
        resourceType: request.resourceType(),
        started: ++sequence,
        routed: 0,
        continued: 0,
        responded: 0,
        finished: 0,
        failed: false,
        responses: 0,
        status: -1,
        headersEmpty: false,
        audioContentType: false,
        bodyBytes: null,
        bodyVerified: false,
        verifiedAt: 0,
        requestRange: request.headers().range,
        diagnostic: {
          source: target!.audio.findIndex(
            (audio) => new URL(audio.path, origin).href === request.url(),
          ),
          start: timing.phase(),
          response: -1,
          failure: -1,
          finish: -1,
          time: -1,
          receipt,
          responseReceipt: -1,
          failureReceipt: -1,
          finishReceipt: -1,
          end: -1,
          network: 0,
          body: 0,
        },
      });
    } catch {
      counts.invalid++;
    }
  });
  context.on("requestfailed", (request) => {
    const start = starts.get(request);
    if (!start) counts.invalid++;
    else if (start.armed) counts.workflow_failures++;
    else counts.preparation_failures++;
    const capture = captures.get(request);
    if (capture) {
      capture.diagnostic.failureReceipt = Date.now();
      refreshAudioTiming(request, capture);
      capture.failed = true;
      capture.diagnostic.failure = timing.phase();
      const reason = request.failure()?.errorText;
      capture.diagnostic.network =
        reason === "net::ERR_ABORTED"
          ? 1
          : reason === "NS_BINDING_ABORTED"
            ? 2
            : reason === "net::ERR_FAILED"
              ? 3
              : 9;
    }
    active.delete(request);
  });
  context.on("requestfinished", (request) => {
    if (!starts.has(request)) counts.invalid++;
    const capture = captures.get(request);
    if (capture) {
      capture.diagnostic.finishReceipt = Date.now();
      refreshAudioTiming(request, capture);
      capture.finished = ++sequence;
      capture.diagnostic.finish = timing.phase();
    }
    active.delete(request);
  });

  async function response(response: Response) {
    const request = response.request();
    const start = starts.get(request);
    if (!start) {
      counts.invalid++;
      return;
    }
    const capture = captures.get(request);
    if (capture) {
      capture.diagnostic.responseReceipt = Date.now();
      refreshAudioTiming(request, capture);
      capture.responses++;
      capture.responded = ++sequence;
      capture.diagnostic.response = timing.phase();
      capture.status = response.status();
      const headers = response.headers();
      capture.headersEmpty = Object.keys(headers).length === 0;
      capture.audioContentType =
        headers["content-type"]?.startsWith("audio/wav") ?? false;
      capture.range = headers["content-range"];
      capture.contentLength = headers["content-length"];
      try {
        if ([200, 206].includes(capture.status))
          capture.requestRange = (await request.allHeaders()).range;
        const limit = Math.max(
          ...target!.audio.map((source) => source.bytes.length),
        );
        const length = headers["content-length"];
        if (
          length !== undefined &&
          (!/^\d+$/.test(length) || Number(length) > limit)
        ) {
          counts.invalid++;
          return;
        }
        const bytes = await response.body();
        capture.bodyBytes = bytes.length;
        counts.audio_bytes += bytes.length;
        if (bytes.length > limit || counts.audio_bytes > limit * 12) {
          counts.invalid++;
          return;
        }
        capture.bytes = bytes;
        const source = target!.audio.find(
          (source) => new URL(source.path, origin).href === capture.url,
        );
        if (source && verifiedAudiobookAudio(capture, source.bytes))
          capture.verifiedAt = ++sequence;
      } catch (error) {
        const message = error instanceof Error ? error.message : "";
        const exact =
          "Protocol error (Network.getResponseBody): Missing content of resource for given requestId";
        capture.bodyReadFailure =
          message === exact || message === `response.body: ${exact}`
            ? "no_resource_data"
            : "other";
        capture.diagnostic.body = audioBodyDiagnostic(message);
      }
      return;
    }
    const path = new URL(response.url()).pathname;
    if (
      !path.startsWith("/api/") &&
      !request.isNavigationRequest() &&
      request.headers()["x-inertia"] !== "true"
    )
      return;
    try {
      const length = response.headers()["content-length"];
      if (
        length !== undefined &&
        (!/^\d+$/.test(length) || Number(length) > 524_288)
      )
        throw new Error("Response bound");
      if (
        response.status() >= 300 &&
        response.status() < 400 &&
        response.status() !== 304
      ) {
        if (start.armed || length !== "0")
          throw new Error("Unexpected redirect");
        return;
      }
      const bytes = await response.body();
      counts.response_bytes += bytes.length;
      if (bytes.length > 524_288 || counts.response_bytes > 4_194_304)
        throw new Error("Response bound");
      if (!start.armed) {
        if (response.status() >= 400) counts.preparation_status_errors++;
        return;
      }
      const value: unknown = JSON.parse(bytes.toString());
      if (start.kind === "artwork") {
        if (
          response.status() !== 404 ||
          !isDeepStrictEqual(value, { error: "Resource not found" })
        )
          throw new Error("Artwork response");
        return;
      }
      if (response.status() !== 200) throw new Error("Workflow status");
      if (start.kind === "progress") {
        if (
          response.headers()["cache-control"] !== "private, no-store" ||
          !start.batch ||
          !isDeepStrictEqual(value, {
            acknowledged: [start.batch.operations[0].id],
          })
        )
          throw new Error("Progress receipt");
        accepted.push(start.batch);
        counts.progress_accepted++;
      } else if (start.kind === "book") {
        if (
          response.headers()["cache-control"] !== "private, no-store" ||
          !isDeepStrictEqual(value, target!.book)
        )
          throw new Error("Book identity");
      } else if (start.kind === "source") {
        const index = target!.recordings.findIndex(
          (id) => path === `/api/v1/recordings/${id}/sources`,
        );
        if (!isDeepStrictEqual(value, target!.sources[index]))
          throw new Error("Source identity");
      } else if (start.kind === "availability") {
        if (!isDeepStrictEqual(value, { can_fetch: false }))
          throw new Error("Artwork provider");
      } else if (start.kind === "navigation") {
        const inertia = value as {
          component: string;
          props: { user: Schema["User"]; detail?: Schema["AudiobookDetail"] };
        };
        if (
          inertia.props.user.id !== target!.account ||
          (path.endsWith(target!.book.playlist.id) &&
            (inertia.component !== "AudiobookPage" ||
              !isDeepStrictEqual(inertia.props.detail, target!.book)))
        )
          throw new Error("Navigation identity");
      } else throw new Error("Unexpected response class");
    } catch {
      if (start.armed) counts.invalid++;
      else counts.preparation_body_failures++;
    }
  }
  context.on("response", (value) => {
    void track(response(value));
  });
  await context.route("**/*", async (route) => {
    const request = route.request();
    try {
      const start = starts.get(request);
      if (
        !start ||
        start.kind === "forbidden" ||
        counts.preparation_requests + counts.workflow_requests > 300
      ) {
        await route.abort();
        return;
      }
      if (start.kind === "progress") {
        const operation = start.batch?.operations[0];
        if (
          !operation ||
          operation.action.kind !== "progress_set" ||
          !target ||
          !(await progress.matches(
            request,
            operation.id,
            operation.action,
            target.audio.map((audio, index) => ({
              path: audio.path,
              recording: target!.recordings[index],
            })),
          ))
        ) {
          counts.invalid++;
          await route.abort();
          return;
        }
      }
      const capture = captures.get(request);
      if (capture) capture.routed = ++sequence;
      await route.continue();
      if (capture) capture.continued = ++sequence;
    } catch {
      counts.invalid++;
    }
  });

  await captureConsolePrivacy(
    context,
    (text, capture) => {
      counts.console_bytes += Buffer.byteLength(text);
      if (counts.console_bytes <= 524_288) texts.push(text);
      else counts.malformed_console++;
      if (
        privateValues.some((value) => value.length >= 4 && text.includes(value))
      )
        counts.private_console++;
      if (text.includes("owned audiobook playback console probe"))
        counts[capture === "text" ? "probe_text" : "probe_snapshot"]++;
      if (capture !== "snapshot") return;
      try {
        const args: unknown = JSON.parse(text);
        if (!Array.isArray(args) || args[0] !== "[hito]") return;
        const event = args[1] as Record<string, unknown>;
        if (
          !event ||
          ![
            "audiobooks.playback_requested",
            "audiobooks.playback_finished",
          ].includes(String(event.event))
        )
          return;
        if (
          Object.keys(event).some(
            (key) =>
              ![
                "event",
                "request_id",
                "action",
                "mode",
                "reason",
                "count",
                "duration_ms",
              ].includes(key),
          ) ||
          typeof event.request_id !== "string" ||
          !uuid.test(event.request_id) ||
          !["chapter", "bookmark"].includes(String(event.action)) ||
          event.mode !== "browser"
        )
          throw new Error("Playback log envelope");
        if (event.event === "audiobooks.playback_requested") {
          if (logs.has(event.request_id) || logs.size >= 2)
            throw new Error("Duplicate playback run");
          logs.set(event.request_id, String(event.action));
          counts.playback_started++;
        } else {
          if (
            logs.get(event.request_id) !== event.action ||
            event.reason !== "handed_off" ||
            event.count !== 3 ||
            typeof event.duration_ms !== "number" ||
            !Number.isFinite(event.duration_ms) ||
            event.duration_ms < 0
          )
            throw new Error("Playback log outcome");
          logs.delete(event.request_id);
          counts.playback_finished++;
          counts[
            event.action === "chapter"
              ? "chapter_handoffs"
              : "bookmark_handoffs"
          ]++;
        }
      } catch {
        counts.malformed_console++;
      }
    },
    () => counts.malformed_console++,
    pending,
  );
  await context.addInitScript((expected) => {
    if (location.origin === expected)
      localStorage.setItem("hito-log-level", "debug");
  }, origin);

  const audioSources = () =>
    target?.audio.map((source) => ({
      url: new URL(source.path, origin).href,
      bytes: source.bytes,
    })) ?? [];
  const audio = () => {
    const audit = auditAudiobookAudio(
      [...captures.values()],
      audioSources(),
      browser,
      seekWitnesses,
    );
    return {
      ...audit,
      unexplained_workflow_failures:
        counts.workflow_failures - audit.explained_seek_cancellations,
    };
  };
  return {
    counts,
    audio,
    pending: () => pending.size + active.size,
    accepted: () => accepted.slice(),
    audioEvidence: () => ({
      records: [...captures.values()],
      sources: audioSources(),
      witnesses: seekWitnesses.slice(),
    }),
    async seekCheckpoint(page: Page, path: string, position: number) {
      if (
        !target ||
        seekArmed ||
        !target.audio.some((audio) => audio.path === path) ||
        !Number.isSafeInteger(position) ||
        position <= 0 ||
        position >= 90_000
      )
        throw new Error("The owned bookmark seek checkpoint was invalid");
      seekArmed = true;
      const checkpoint = await audiobookSeekCheckpoint(page, path, position);
      seekHandles.add(checkpoint);
      let finished = false;
      return {
        async ready() {
          const witness = await checkpoint.read();
          return (
            !witness.changed &&
            !witness.overflow &&
            witness.events.length === 5 &&
            witness.events[4].position > position + 10
          );
        },
        async finish() {
          if (finished)
            throw new Error(
              "The bookmark seek checkpoint was already consumed",
            );
          finished = true;
          try {
            seekWitnesses.push(await checkpoint.read());
          } finally {
            seekHandles.delete(checkpoint);
            await checkpoint.dispose();
          }
        },
      };
    },
    async pauseCheckpoint(page: Page, path: string) {
      const pause = await progress.pauseCheckpoint(page, path);
      let used = false;
      return async () => {
        if (used) throw new Error("The pause checkpoint was already consumed");
        used = true;
        try {
          await expect
            .poll(async () => {
              const witness = await pause.read();
              return (
                witness !== null &&
                accepted.some(
                  (batch) => batch.operations[0].id === witness.operation,
                )
              );
            })
            .toBe(true);
          await expect.poll(() => pending.size + active.size).toBe(0);
          if (!(await pause.read()))
            throw new Error("The acknowledged pause witness was unavailable");
          return await audiobookMediaState(page, path);
        } finally {
          await pause.dispose();
        }
      };
    },
    async checkpoint(phase: number) {
      await timing.checkpoint(
        phase,
        counts.workflow_failures,
        [...captures.values()].filter(
          (capture) => capture.bodyReadFailure !== undefined,
        ).length,
        [...active].filter((request) => captures.has(request)).length,
      );
    },
    diagnostics() {
      const requests = [...captures.values()].map((capture, index) => {
        const requested = capture.requestRange?.match(/^bytes=(\d*)-(\d*)$/);
        const returned = capture.range?.match(/^bytes (\d+)-(\d+)\/(\d+)$/);
        const networkTime = timing.relative(capture.diagnostic.time);
        const receipt = timing.relative(capture.diagnostic.receipt);
        const time = networkTime >= 0 ? networkTime : receipt;
        const safe = (value: string | undefined) =>
          value && /^\d+$/.test(value) && Number.isSafeInteger(Number(value))
            ? Number(value)
            : -1;
        return [
          index + 1,
          capture.diagnostic.source,
          time,
          capture.diagnostic.start,
          capture.diagnostic.response,
          capture.diagnostic.failure,
          capture.diagnostic.finish,
          capture.resourceType === "media"
            ? 1
            : capture.resourceType === "other"
              ? 2
              : 3,
          capture.status,
          safe(requested?.[1]),
          safe(requested?.[2]),
          safe(returned?.[1]),
          safe(returned?.[2]),
          capture.bodyBytes ?? -1,
          capture.diagnostic.network,
          capture.diagnostic.body,
          receipt,
          networkTime >= 0 ? 1 : receipt >= 0 ? 2 : 0,
          timing.relative(capture.diagnostic.responseReceipt),
          timing.relative(capture.diagnostic.failureReceipt),
          timing.relative(capture.diagnostic.finishReceipt),
          timing.relative(capture.diagnostic.end),
        ];
      });
      return {
        overflow: timing.overflow(),
        requests,
        checkpoints: timing.checkpoints(),
        seek: seekWitnesses.map((witness) => ({
          changed: Number(witness.changed),
          overflow: Number(witness.overflow),
          observed_until: timing.relative(witness.observedUntil),
          events: witness.events.map((event) => [
            event.kind,
            event.owner,
            timing.relative(event.time),
            event.position,
            Number(event.trusted),
            Number(event.connected),
            Number(event.active),
            Number(event.unique),
          ]),
        })),
      };
    },
    async arm(value: Target) {
      if (target || active.size || pending.size)
        throw new Error("The playback boundary was not quiet");
      target = value;
      await progress.arm();
      await timing.arm();
    },
    async finish() {
      for (const checkpoint of seekHandles) {
        try {
          await checkpoint.dispose();
        } catch {
          counts.invalid++;
        }
        seekHandles.delete(checkpoint);
      }
      const deadline = setTimeout(() => {
        counts.invalid++;
      }, 10_000);
      const expires = Date.now() + 10_000;
      while (pending.size && Date.now() < expires)
        await Promise.race([
          Promise.allSettled([...pending]),
          new Promise((resolve) => setTimeout(resolve, 50)),
        ]);
      clearTimeout(deadline);
      if (pending.size || active.size || logs.size) counts.invalid++;
      for (const text of texts)
        if (
          privateValues.some(
            (value) => value.length >= 4 && text.includes(value),
          )
        )
          counts.private_console++;
    },
  };
}
