import type {
  BrowserContext,
  Page,
  Request as BrowserRequest,
} from "@playwright/test";
import type { components } from "../../src/generated";

type Progress = Extract<
  components["schemas"]["Action"],
  { kind: "progress_set" }
>;
type Sample = {
  operation: string;
  source: string;
  position: number;
  speed: number;
  duration: number;
  time: number;
  exact: boolean;
};
type Observed = {
  enabled: boolean;
  total: number;
  invalid: number;
  samples: Sample[];
  pause: PauseCapture | null;
  pauseUsed: boolean;
};
type PauseCapture = {
  audio: HTMLAudioElement;
  source: string;
  operation: string;
  count: number;
  invalid: boolean;
  position: number;
  speed: number;
  time: number;
};

export async function audiobookProgressObservation(
  context: BrowserContext,
  origin: string,
) {
  await context.addInitScript((origin) => {
    if (location.origin !== origin) return;
    const target = globalThis as unknown as Record<string, unknown>;
    const observed: Observed = {
      enabled: false,
      total: 0,
      invalid: 0,
      samples: [],
      pause: null,
      pauseUsed: false,
    };
    target.__hitoOwnedPlaybackProgress = observed;
    const native = globalThis.fetch.bind(globalThis);
    globalThis.fetch = (input, options) => {
      if (observed.enabled) {
        try {
          const url = new URL(
            input instanceof Request ? input.url : String(input),
            location.href,
          );
          const method =
            options?.method ??
            (input instanceof Request ? input.method : "GET");
          if (
            url.origin === origin &&
            url.pathname === "/api/v1/sync" &&
            method === "POST"
          ) {
            if (
              typeof options?.body !== "string" ||
              options.body.length > 16_384
            )
              throw new Error("The progress submission could not be observed");
            const batch = JSON.parse(options.body);
            const operation = batch.operations?.[0];
            if (operation?.action?.kind === "progress_set") {
              if (
                ++observed.total > 64 ||
                observed.samples.length >= 64 ||
                typeof operation.id !== "string" ||
                observed.samples.some(
                  (sample) => sample.operation === operation.id,
                )
              )
                throw new Error("The progress observation budget was exceeded");
              const selected = document.querySelectorAll<HTMLAudioElement>(
                "audio[data-hito-player]",
              );
              const audio = selected[0];
              const sample: Sample = {
                operation: operation.id,
                source: audio?.currentSrc ?? "",
                position: audio ? Math.floor(audio.currentTime * 1000) : -1,
                speed: audio?.playbackRate ?? -1,
                duration: audio?.duration ?? -1,
                time: performance.now(),
                exact:
                  selected.length === 1 &&
                  audio.isConnected &&
                  audio.error === null &&
                  new URL(audio.getAttribute("src") ?? "", location.href)
                    .href === audio.currentSrc,
              };
              observed.samples.push(sample);
              const pause = observed.pause;
              if (
                pause &&
                pause.count === 1 &&
                !pause.invalid &&
                !pause.operation &&
                sample.exact &&
                audio === pause.audio &&
                audio.paused &&
                !audio.ended &&
                sample.source === pause.source &&
                sample.position === pause.position &&
                sample.speed === pause.speed &&
                sample.time >= pause.time &&
                sample.time - pause.time <= 10_000
              )
                pause.operation = sample.operation;
            }
          }
        } catch {
          observed.invalid++;
        }
      }
      return native(input, options);
    };
  }, origin);
  return {
    async pauseCheckpoint(page: Page, path: string) {
      const handle = await page.evaluateHandle((path) => {
        const observed = (globalThis as unknown as Record<string, unknown>)
          .__hitoOwnedPlaybackProgress as Observed;
        const selected = document.querySelectorAll<HTMLAudioElement>(
          "audio[data-hito-player]",
        );
        const audio = selected[0];
        const source = new URL(path, location.href).href;
        if (
          !observed?.enabled ||
          observed.invalid ||
          observed.pauseUsed ||
          selected.length !== 1 ||
          !audio.isConnected ||
          audio.paused ||
          audio.ended ||
          audio.error !== null ||
          audio.currentSrc !== source ||
          new URL(audio.getAttribute("src") ?? "", location.href).href !==
            source
        )
          throw new Error(
            "The pause checkpoint needs the actual playing element",
          );
        observed.pauseUsed = true;
        const pause: PauseCapture = {
          audio,
          source,
          operation: "",
          count: 0,
          invalid: false,
          position: -1,
          speed: -1,
          time: -1,
        };
        observed.pause = pause;
        const exact = () => {
          const selected = document.querySelectorAll("audio[data-hito-player]");
          return (
            selected.length === 1 &&
            selected[0] === audio &&
            audio.isConnected &&
            audio.error === null &&
            !audio.ended &&
            audio.currentSrc === source &&
            new URL(audio.getAttribute("src") ?? "", location.href).href ===
              source
          );
        };
        const record = (event: Event) => {
          if (
            ++pause.count !== 1 ||
            !event.isTrusted ||
            event.target !== audio ||
            !exact() ||
            !audio.paused
          )
            pause.invalid = true;
          pause.position = Math.floor(audio.currentTime * 1000);
          pause.speed = audio.playbackRate;
          pause.time = performance.now();
          if (
            !Number.isSafeInteger(pause.position) ||
            pause.position <= 0 ||
            pause.position > 90_000 ||
            !Number.isFinite(pause.speed)
          )
            pause.invalid = true;
        };
        const invalidate = () => {
          pause.invalid = true;
        };
        const interruptions = [
          "loadstart",
          "emptied",
          "play",
          "ended",
          "error",
        ];
        audio.addEventListener("pause", record, true);
        interruptions.forEach((event) =>
          audio.addEventListener(event, invalidate),
        );
        const inspect = (records: MutationRecord[]) => {
          if (
            records.some(
              (record) =>
                (record.type === "attributes" && record.target === audio) ||
                [...record.removedNodes].some(
                  (node) => node === audio || node.contains(audio),
                ),
            )
          )
            pause.invalid = true;
        };
        const mutations = new MutationObserver(inspect);
        mutations.observe(document.body, {
          childList: true,
          subtree: true,
          attributes: true,
          attributeFilter: ["src", "data-hito-player"],
        });
        return {
          read() {
            inspect(mutations.takeRecords());
            if (pause.invalid || observed.invalid || !exact())
              throw new Error("The actual pause witness changed");
            if (pause.count !== 1 || !pause.operation) return null;
            if (
              !audio.paused ||
              Math.floor(audio.currentTime * 1000) !== pause.position ||
              audio.playbackRate !== pause.speed
            )
              throw new Error("The paused media position changed");
            return {
              operation: pause.operation,
              position: pause.position,
              speed: pause.speed,
            };
          },
          close() {
            mutations.disconnect();
            audio.removeEventListener("pause", record, true);
            interruptions.forEach((event) =>
              audio.removeEventListener(event, invalidate),
            );
            observed.pause = null;
          },
        };
      }, path);
      return {
        read: () => handle.evaluate((witness) => witness.read()),
        async dispose() {
          try {
            await handle.evaluate((witness) => witness.close());
          } finally {
            await handle.dispose();
          }
        },
      };
    },
    async arm() {
      const pages = context
        .pages()
        .filter((page) => new URL(page.url()).origin === origin);
      if (pages.length !== 1)
        throw new Error("The progress observation needs one owned page");
      await pages[0].evaluate(() => {
        const observed = (globalThis as unknown as Record<string, unknown>)
          .__hitoOwnedPlaybackProgress as Observed;
        if (!observed || observed.enabled)
          throw new Error("The progress observation could not be armed");
        observed.enabled = true;
      });
    },
    async matches(
      request: BrowserRequest,
      operation: string,
      progress: Progress,
      sources: { path: string; recording: string }[],
    ) {
      try {
        const result = await request
          .frame()
          .page()
          .evaluate((operation) => {
            const observed = (globalThis as unknown as Record<string, unknown>)
              .__hitoOwnedPlaybackProgress as Observed;
            const index =
              observed?.samples.findIndex(
                (sample) => sample.operation === operation,
              ) ?? -1;
            if (!observed?.enabled || observed.invalid || index < 0)
              return null;
            return {
              sample: observed.samples.splice(index, 1)[0],
              now: performance.now(),
            };
          }, operation);
        if (!result) return false;
        const { sample, now } = result;
        const source = sources.find(
          (source) => new URL(source.path, origin).href === sample.source,
        );
        return (
          sample.exact &&
          source?.recording === progress.recording_id &&
          Number.isSafeInteger(sample.position) &&
          sample.position > 0 &&
          sample.position <= 90_000 &&
          Number.isFinite(sample.duration) &&
          Math.abs(sample.duration - 90) < 0.1 &&
          sample.speed === progress.speed &&
          Math.abs(sample.position - progress.position_ms) <= 250 &&
          Number.isFinite(sample.time) &&
          now >= sample.time &&
          now - sample.time <= 10_000
        );
      } catch {
        return false;
      }
    },
  };
}
