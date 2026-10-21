import { isDeepStrictEqual } from "node:util";
import { expect, type Page, type Route } from "@playwright/test";
import type { components } from "../../src/generated";
import type { OutageProvider } from "./providerOutageProvider";
import type { OutageLibrary } from "./providerOutageLibrary";
import { outageMessage, type OutageEvidence } from "./providerOutageEvidence";

type Schema = components["schemas"];
export type OutagePlaybackPhase =
  | "accepted_lost_ack"
  | "fresh_play"
  | "recovered_playing"
  | "pcm_decode"
  | "navigation"
  | "navigation_retained"
  | "pause_recovery"
  | "queue_start"
  | "queue_outage"
  | "queue_ui"
  | "queue_seek"
  | "queue_ended"
  | "queue_advance"
  | "pause_queue";

export async function outageMediaFacts(page: Page) {
  return page.evaluate(() => {
    const audio = document.querySelector<HTMLAudioElement>(
      "audio[data-hito-player]",
    );
    if (!audio) return { audio_present: false as const };
    const source = audio.currentSrc || audio.getAttribute("src") || "";
    let sourceScheme: "http" | "https" | "blob" | "none" | "other" = "none";
    if (source) {
      try {
        const protocol = new URL(source, document.baseURI).protocol;
        sourceScheme =
          protocol === "http:"
            ? "http"
            : protocol === "https:"
              ? "https"
              : protocol === "blob:"
                ? "blob"
                : "other";
      } catch {
        sourceScheme = "other";
      }
    }
    return {
      audio_present: true as const,
      connected: audio.isConnected,
      paused: audio.paused,
      ended: audio.ended,
      duration: Number.isFinite(audio.duration) ? audio.duration : null,
      current_time: Number.isFinite(audio.currentTime)
        ? audio.currentTime
        : null,
      ready_state: audio.readyState,
      network_state: audio.networkState,
      error_code: audio.error?.code ?? null,
      source_scheme: sourceScheme,
    };
  });
}
export type OutageMediaFacts = Awaited<ReturnType<typeof outageMediaFacts>>;

const activeAudio = (page: Page) => page.locator("audio[data-hito-player]");
const dock = (page: Page) =>
  page.getByRole("contentinfo", { name: "Audio player", exact: true });

async function playing(page: Page) {
  await expect
    .poll(
      () =>
        activeAudio(page).evaluate(
          (audio: HTMLAudioElement) =>
            !audio.paused &&
            audio.error === null &&
            audio.currentTime > 0.3 &&
            Number.isFinite(audio.duration) &&
            Math.abs(audio.duration - 90) < 0.03,
        ),
      { timeout: 10_000 },
    )
    .toBe(true);
}

async function pause(page: Page) {
  await Promise.all([
    page.waitForResponse(
      (response) =>
        new URL(response.url()).pathname === "/api/v1/sync" &&
        response.request().method() === "POST" &&
        response.status() === 200,
      { timeout: 10_000 },
    ),
    dock(page).getByRole("button", { name: "Pause", exact: true }).click(),
  ]);
  expect(
    await activeAudio(page).evaluate(
      (audio: HTMLAudioElement) => audio.paused && audio.error === null,
    ),
  ).toBe(true);
}

export async function playThroughOutage(
  page: Page,
  library: OutageLibrary,
  provider: OutageProvider,
  evidence: OutageEvidence,
  checkpoint: (phase: OutagePlaybackPhase) => void,
) {
  checkpoint("accepted_lost_ack");
  provider.setAvailability("retry", "unavailable");
  await page.goto(`/playlists/${library.copies.retry.playlist.id}`);
  const path = `/api/v1/recordings/${library.ids.retry}/sources/resolve`;
  let accepted: Schema["SourceResolution"] | null = null;
  let delivered: Schema["SourceResolution"] | null = null;
  let providerCallsAfterAcceptance = 0;
  const attempts: string[] = [];
  let invalid = false;
  const intercept = async (route: Route) => {
    if (route.request().method() !== "POST") return route.continue();
    const raw = route.request().postData();
    if (!raw || Buffer.byteLength(raw) > 4096 || attempts.length >= 2) {
      invalid = true;
      return route.abort("failed");
    }
    attempts.push(raw);
    if (attempts.length === 2) return route.continue();
    try {
      const response = await route.fetch({ timeout: 10_000 });
      try {
        const bytes = await response.body();
        if (response.status() !== 200 || bytes.length > 16_384)
          throw new Error("Owned accepted response rejected");
        accepted = JSON.parse(bytes.toString()) as Schema["SourceResolution"];
        providerCallsAfterAcceptance = provider.counts.sources;
      } finally {
        await response.dispose();
      }
    } catch {
      invalid = true;
    }
    await route.abort("failed");
  };
  await page.route(`**${path}`, intercept);
  try {
    const response = page.waitForResponse(
      (response) =>
        new URL(response.url()).pathname === path && response.status() === 200,
      { timeout: 15_000 },
    );
    await page
      .getByRole("button", { name: "Play playlist", exact: true })
      .click();
    const bytes = await (await response).body();
    if (bytes.length > 16_384)
      throw new Error("Owned retry receipt exceeded its bound");
    delivered = JSON.parse(bytes.toString()) as Schema["SourceResolution"];
    await expect(dock(page).getByRole("status")).toHaveText(outageMessage);
    const receipt = accepted as Schema["SourceResolution"] | null;
    expect(
      !invalid &&
        attempts.length === 2 &&
        attempts[0] === attempts[1] &&
        receipt?.outcome === "provider_unavailable" &&
        receipt.sources.length === 0 &&
        isDeepStrictEqual(receipt, delivered) &&
        provider.counts.sources === providerCallsAfterAcceptance,
    ).toBe(true);
    expect(await activeAudio(page).getAttribute("src")).toBeNull();
  } finally {
    await page.unroute(`**${path}`, intercept);
  }
  checkpoint("fresh_play");
  const failedRequest = JSON.parse(attempts[0]) as Schema["ResolveSource"];
  provider.setAvailability("retry", "ready");
  const response = page.waitForResponse(
    (response) =>
      new URL(response.url()).pathname === path && response.status() === 200,
    { timeout: 10_000 },
  );
  await page
    .getByRole("button", { name: "Play playlist", exact: true })
    .click();
  const readyResponse = await response;
  const bytes = await readyResponse.body();
  if (bytes.length > 16_384)
    throw new Error("Owned ready receipt exceeded its bound");
  const ready = JSON.parse(bytes.toString()) as Schema["SourceResolution"];
  const fresh = readyResponse
    .request()
    .postDataJSON() as Schema["ResolveSource"];
  expect(
    ready.outcome === "ready" &&
      ready.sources.length === 1 &&
      ready.request_id === fresh.request_id &&
      fresh.request_id !== failedRequest.request_id &&
      ready.resolution_id !== delivered?.resolution_id,
  ).toBe(true);
  checkpoint("recovered_playing");
  await playing(page);
  checkpoint("pcm_decode");
  const pcm = await activeAudio(page).evaluate(
    async (audio: HTMLAudioElement) => {
      const context = new AudioContext({ sampleRate: 16_000 });
      try {
        const response = await fetch(audio.src, { credentials: "same-origin" });
        if (!response.ok) return false;
        const bytes = await response.arrayBuffer();
        if (bytes.byteLength > 4 * 1024 * 1024) return false;
        const decoded = await context.decodeAudioData(bytes);
        const samples = decoded.getChannelData(0);
        return (
          decoded.numberOfChannels === 1 &&
          decoded.sampleRate === 16_000 &&
          decoded.length === 1_440_000 &&
          Math.abs(decoded.duration - 90) < 0.001 &&
          samples
            .slice(0, 1024)
            .every(
              (sample, index) =>
                Math.abs(
                  sample -
                    Math.round(
                      Math.sin((2 * Math.PI * 440 * index) / 16_000) * 4096,
                    ) /
                      32768,
                ) < 0.0001,
            )
        );
      } finally {
        await context.close();
      }
    },
  );
  expect(pcm).toBe(true);
  checkpoint("navigation");
  const handle = await activeAudio(page).elementHandle();
  if (!handle) throw new Error("Owned playing media is missing");
  const before = await handle.evaluate(
    (element) => (element as HTMLAudioElement).currentTime,
  );
  await page
    .getByRole("navigation", { name: "Main navigation", exact: true })
    .getByRole("link", { name: "Library", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "Your library", exact: true }),
  ).toBeVisible();
  checkpoint("navigation_retained");
  await expect
    .poll(() =>
      handle.evaluate((element, previousTime) => {
        const audio = element as HTMLAudioElement;
        return (
          audio.isConnected &&
          audio === document.querySelector("audio[data-hito-player]") &&
          !audio.paused &&
          audio.error === null &&
          audio.currentTime > previousTime
        );
      }, before),
    )
    .toBe(true);
  checkpoint("pause_recovery");
  await pause(page);
  checkpoint("queue_start");
  await page.goto(`/playlists/${library.copies.queue.playlist.id}`);
  const queuePath = `/api/v1/recordings/${library.ids.queue}/sources/resolve`;
  const queueResponse = page.waitForResponse(
    (response) =>
      new URL(response.url()).pathname === queuePath &&
      response.status() === 200,
    { timeout: 10_000 },
  );
  await page
    .getByRole("button", { name: "Play playlist", exact: true })
    .click();
  await playing(page);
  checkpoint("queue_outage");
  const queueBytes = await (await queueResponse).body();
  if (queueBytes.length > 16_384)
    throw new Error("Owned queue receipt exceeded its bound");
  const queueReceipt = JSON.parse(
    queueBytes.toString(),
  ) as Schema["SourceResolution"];
  expect(
    queueReceipt.outcome === "provider_unavailable" &&
      queueReceipt.sources.length === 0,
  ).toBe(true);
  expect(
    await activeAudio(page).evaluate(
      (audio: HTMLAudioElement) =>
        !audio.paused && audio.currentTime < 10 && audio.error === null,
    ),
  ).toBe(true);
  await expect(dock(page).getByRole("status")).toHaveCount(0);
  checkpoint("queue_ui");
  await dock(page)
    .getByRole("button", { name: "Open queue", exact: true })
    .click();
  const queue = page.getByRole("complementary", {
    name: "Playback queue",
    exact: true,
  });
  await expect(queue.locator("li")).toHaveCount(3);
  await expect(queue.locator("li.current")).toHaveCount(1);
  expect(
    await queue
      .locator("li")
      .evaluateAll((rows) => rows[0].classList.contains("current")),
  ).toBe(true);
  checkpoint("queue_seek");
  const queueAudio = await activeAudio(page).elementHandle();
  if (!queueAudio) throw new Error("Owned queue media is missing");
  await queueAudio.evaluate((audio) => {
    const target = audio as HTMLAudioElement & { __ownedEnded?: number };
    target.__ownedEnded = 0;
    audio.addEventListener("ended", () => {
      target.__ownedEnded = (target.__ownedEnded ?? 0) + 1;
    });
  });
  const slider = dock(page).getByRole("slider", { name: "Seek", exact: true });
  const box = await slider.boundingBox();
  if (!box) throw new Error("Owned seek control is missing");
  await slider.click({ position: { x: box.width * 0.98, y: box.height / 2 } });
  checkpoint("queue_ended");
  await expect
    .poll(() =>
      queueAudio.evaluate(
        (audio) =>
          (audio as HTMLAudioElement & { __ownedEnded?: number }).__ownedEnded,
      ),
    )
    .toBe(1);
  checkpoint("queue_advance");
  await expect
    .poll(() =>
      queue
        .locator("li")
        .evaluateAll((rows) => rows[2].classList.contains("current")),
    )
    .toBe(true);
  await playing(page);
  await expect(dock(page).getByRole("status")).toHaveText(outageMessage);
  expect(
    evidence.requests.filter(
      (request) => request.recording === library.ids.queue,
    ).length,
  ).toBe(1);
  expect(
    evidence.requests.filter(
      (request) => request.recording === library.ids.retry,
    ).length,
  ).toBe(3);
  checkpoint("pause_queue");
  await pause(page);
  return {
    lost_acknowledgements: 1,
    retry_wire_attempts: attempts.length,
    accepted_unavailable_commands: 2,
    exact_retry_bytes: true,
    fresh_explicit_request: true,
    pcm_decoded: true,
    navigation_retained_audio: true,
    actual_queue_ended: 1,
  };
}
