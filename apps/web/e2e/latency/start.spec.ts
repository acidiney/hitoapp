import {
  expect,
  test,
  webkit,
  type BrowserContext,
  type Page,
} from "@playwright/test";
import { startProvider } from "./fake-provider.mjs";
import type { Provider } from "./fake-provider.mjs";
import type { Schema } from "../../src/shared/schema";
import { installProbe } from "./probe";
import {
  libraryTracks,
  prepareOutput,
  record,
  rttMilliseconds,
  sampleCount,
  warmupCount,
  writeEnvironment,
} from "./results";
import {
  connectProvider,
  credentials,
  prepareLibrary,
  recordingTotal,
  writeHeaders,
} from "./server";

const groupSize = warmupCount + sampleCount;
const formatSpan = Math.floor(libraryTracks / 3);
const groups = {
  remote: 0,
  remoteRtt: 1,
  cached: 2,
  cachedRtt: 3,
  refreshed: 4,
  refreshedRtt: 5,
} as const;

test.describe.configure({ mode: "serial" });

let context: BrowserContext;
let page: Page;
let provider: Provider;
const adopted: Schema<"Recording">[] = [];

const sourceId = (recording: string, side: "a" | "b") =>
  `audio/${recording}/${side}`;
const entry = (id: string) => ({ id, label: "Bench", ready: true });

async function find(title: string) {
  const search = page.getByRole("textbox", { name: "Search library" });
  await search.fill(title);
  await search.press("Enter");
  const play = page.getByRole("button", {
    name: `Play ${title}`,
    exact: true,
  });
  await play.waitFor();
  return play;
}

const conditions = {
  audible: () => window.__latency.state().audible !== null,
  position: () => window.__latency.state().position >= 1.5,
  resumed: () => window.__latency.state().resumed !== null,
};

async function awaitProbe(condition: keyof typeof conditions) {
  await page.waitForFunction(conditions[condition], undefined, {
    timeout: 15_000,
    polling: 20,
  });
}

async function startOnce(title: string) {
  const play = await find(title);
  await page.evaluate(() => window.__latency.arm());
  await play.click();
  await awaitProbe("audible");
  return page.evaluate(() => window.__latency.state());
}

async function silence() {
  await page.evaluate(() => window.__latency.silence());
}

function failure(error: unknown) {
  return String(error instanceof Error ? error.message : error)
    .split("\n")[0]
    .replaceAll(",", ";");
}

test.beforeAll(async ({ browser, baseURL }) => {
  test.setTimeout(30 * 60 * 1000);
  prepareOutput();
  context = await browser.newContext({ baseURL });
  const origin = new URL(baseURL!).origin;
  const scan = await prepareLibrary(context.request, origin);
  const total = await recordingTotal(context.request);
  provider = await startProvider({
    audioFile: "/music/remote/remote.mp3",
    recordings: groupSize * 6,
  });
  const adopt = await connectProvider(context.request, origin, provider);
  for (const [index, item] of provider.records.entries()) {
    const group = Math.floor(index / groupSize);
    const sides =
      group === groups.cached || group === groups.cachedRtt
        ? [entry(sourceId(item.id, "a")), entry(sourceId(item.id, "b"))]
        : [entry(sourceId(item.id, "a"))];
    provider.setSources(item.id, sides);
    adopted.push(await adopt(item.id));
  }
  page = await context.newPage();
  await page.addInitScript(installProbe);
  await page.goto("/library");
  await expect(
    page.getByRole("textbox", { name: "Search library" }),
  ).toBeVisible();
  writeEnvironment({
    browser: browser.version(),
    userAgent: await page.evaluate(() => navigator.userAgent),
    library_scan_seconds: scan.scanSeconds,
    recordings_api: total,
    provider_recordings: provider.records.length,
    samples: sampleCount,
    warmup: warmupCount,
    rtt_ms: rttMilliseconds,
  });
});

test.afterAll(async () => {
  await provider?.close();
  await context?.close();
});

const localFormats = [
  { format: "FLAC", offset: 0, engine: "chromium", variant: "FLAC" },
  { format: "MP3", offset: 1, engine: "chromium", variant: "MP3" },
  { format: "M4A", offset: 2, engine: "webkit", variant: "M4A (WebKit)" },
] as const;

for (const { format, offset, engine, variant } of localFormats) {
  test(`local ${format} start in ${engine}`, async ({ baseURL }) => {
    const engineBrowser =
      engine === "webkit" ? await webkit.launch({ args: [] }) : null;
    const original = page;
    if (engineBrowser) {
      const alternate = await engineBrowser.newContext({ baseURL });
      const login = await alternate.request.post("/api/v1/login", {
        headers: writeHeaders(new URL(baseURL!).origin),
        data: credentials,
      });
      expect(login.status()).toBe(200);
      page = await alternate.newPage();
      await page.addInitScript(installProbe);
      await page.goto("/library");
    }
    try {
      for (let n = 0; n < groupSize; n++) {
        const index = ((n * 97 + 13) % formatSpan) * 3 + offset;
        const title = `Bench ${format} ${String(index).padStart(5, "0")}`;
        const sample = n - warmupCount;
        try {
          const state = await startOnce(title);
          if (sample >= 0)
            record({
              scenario: "local_start",
              variant,
              rtt: 0,
              sample,
              detail: title,
              ok: true,
              latency: state.audible! - state.clicked!,
              playing: state.playing! - state.clicked!,
              timeupdate:
                state.timeupdate === null
                  ? undefined
                  : state.timeupdate - state.clicked!,
            });
        } catch (error) {
          if (sample >= 0)
            record({
              scenario: "local_start",
              variant,
              rtt: 0,
              sample,
              detail: title,
              ok: false,
              latency: 0,
              note: failure(error),
            });
        }
        await silence();
      }
    } finally {
      if (engineBrowser) {
        await engineBrowser.close();
        page = original;
      }
    }
  });
}

for (const [label, group, rtt] of [
  ["remote_start", groups.remote, 0],
  ["remote_start", groups.remoteRtt, rttMilliseconds],
] as const) {
  test(`ready remote start at ${rtt} ms`, async () => {
    provider.setLatency(rtt);
    for (let n = 0; n < groupSize; n++) {
      const item = adopted[group * groupSize + n];
      const sample = n - warmupCount;
      const before = { ...provider.counters };
      try {
        const state = await startOnce(item.title);
        if (sample >= 0)
          record({
            scenario: label,
            variant: "relay",
            rtt,
            sample,
            detail: "mp3",
            ok: true,
            latency: state.audible! - state.clicked!,
            playing: state.playing! - state.clicked!,
            timeupdate:
              state.timeupdate === null
                ? undefined
                : state.timeupdate - state.clicked!,
            note: `provider_sources=${provider.counters.sources - before.sources};provider_audio=${provider.counters.audio - before.audio}`,
          });
      } catch (error) {
        if (sample >= 0)
          record({
            scenario: label,
            variant: "relay",
            rtt,
            sample,
            detail: "mp3",
            ok: false,
            latency: 0,
            note: failure(error),
          });
      }
      await silence();
    }
    provider.setLatency(0);
  });
}

for (const [variant, group, rtt] of [
  ["cached_alternate", groups.cached, 0],
  ["cached_alternate", groups.cachedRtt, rttMilliseconds],
  ["refreshed_alternate", groups.refreshed, 0],
  ["refreshed_alternate", groups.refreshedRtt, rttMilliseconds],
] as const) {
  test(`${variant} recovery at ${rtt} ms`, async () => {
    for (let n = 0; n < groupSize; n++) {
      const item = adopted[group * groupSize + n];
      const providerId = provider.records[group * groupSize + n].id;
      const a = sourceId(providerId, "a");
      const b = sourceId(providerId, "b");
      const sample = n - warmupCount;
      const refreshed = variant === "refreshed_alternate";
      provider.pace(a);
      if (!refreshed) provider.pace(b);
      provider.setLatency(0);
      const before = { ...provider.counters };
      try {
        await startOnce(item.title);
        await awaitProbe("position");
        let active = a;
        if (refreshed) provider.setSources(providerId, [entry(b)]);
        else {
          if (provider.streamsOpen(a) === 0) active = b;
          provider.unpace(active === a ? b : a);
        }
        provider.setLatency(rtt);
        const dropped = provider.drop(active);
        await awaitProbe("resumed");
        const state = await page.evaluate(() => window.__latency.state());
        if (sample >= 0)
          record({
            scenario: "recovery",
            variant,
            rtt,
            sample,
            detail: "mp3",
            ok: true,
            latency: state.resumed! - dropped,
            detection: state.errored! - dropped,
            playing: state.resumedPlaying! - dropped,
            note: `provider_sources=${provider.counters.sources - before.sources};provider_audio=${provider.counters.audio - before.audio}`,
          });
      } catch (error) {
        if (sample >= 0)
          record({
            scenario: "recovery",
            variant,
            rtt,
            sample,
            detail: "mp3",
            ok: false,
            latency: 0,
            note: failure(error),
          });
      }
      await silence();
      provider.drop(a);
      provider.drop(b);
      provider.setLatency(0);
    }
  });
}
