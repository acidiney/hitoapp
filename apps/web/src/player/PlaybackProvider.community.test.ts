// @vitest-environment jsdom
import { act, cleanup } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { audiobookContinuationMedia } from "../fixtures/audiobookContinuationMedia";
import { audiobookPages } from "../fixtures/audiobooks";
import type { components } from "../generated";
import {
  playbackRequests,
  recording,
  otherRecording,
  renderAccount,
} from "./fixtures/browserMediaSessionPlayback";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

test("browser community retries keep one claim identity and retire held response bodies across accounts in Strict Mode", async () => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  let now = 0;
  vi.spyOn(performance, "now").mockImplementation(() => now);
  const media = audiobookContinuationMedia();
  const claims: { account: string | null; options: RequestInit }[] = [];
  const retiredBody = vi.fn();
  const timedBody = vi.fn();
  playbackRequests([recording], (path, options) => {
    if (!path.endsWith("/successful-playback")) return;
    const account = new Headers(options?.headers).get("x-hito-account");
    claims.push({ account, options: options! });
    if (claims.length === 1) return new Response(null, { status: 503 });
    if (claims.length === 2 || claims.length === 3)
      return new Response(
        new ReadableStream({
          cancel: claims.length === 2 ? retiredBody : timedBody,
        }),
        {
          headers: { "content-type": "application/json" },
        },
      );
    return Response.json("queued");
  });
  const view = renderAccount(true);
  const qualify = async () => {
    await act(async () => {
      await view.player().playQueue([recording]);
    });
    const audio = media.current();
    act(() => {
      media.ready(audio);
      media.observePlaying(audio);
    });
    for (let index = 1; index <= 5; index++) {
      await act(async () => {
        now += 1000;
        audio.currentTime = index;
        audio.dispatchEvent(new Event("timeupdate"));
      });
    }
    return audio;
  };
  try {
    await qualify();
    expect(claims).toHaveLength(2);
    expect(claims[1].options.body).toBe(claims[0].options.body);
    expect(view.player().snapshot()).toMatchObject({
      playing: true,
      error: "",
    });
    const retired = claims[1].options.signal;
    view.rerender(view.accountView("bob"));
    await act(async () => {});
    expect(retired?.aborted).toBe(true);
    expect(retiredBody).toHaveBeenCalledTimes(1);
    expect(claims).toHaveLength(2);
    await qualify();
    expect(claims).toHaveLength(3);
    expect(claims[2].account).toBe("bob");
    expect(claims[2].options.body).not.toBe(claims[0].options.body);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2000);
    });
    expect(timedBody).toHaveBeenCalledTimes(1);
    expect(claims).toHaveLength(4);
    expect(claims[3].options.body).toBe(claims[2].options.body);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(20_000);
    });
    expect(claims).toHaveLength(4);
    expect(view.player().snapshot()).toMatchObject({
      playing: true,
      error: "",
    });
    await act(async () => {
      await view.player().next();
    });
    expect(view.player().snapshot().current).toBeNull();
  } finally {
    view.unmount();
  }
});

test("browser community success requires five advancing ready seconds and excludes restore, seek, pause, stalls and invalid clocks", async () => {
  let now = 0;
  vi.spyOn(performance, "now").mockImplementation(() => now);
  const media = audiobookContinuationMedia();
  const claims: { path: string; options: RequestInit }[] = [];
  playbackRequests([recording], (path, options) => {
    if (!path.endsWith("/successful-playback")) return;
    claims.push({ path, options: options! });
    return Response.json("queued");
  });
  const view = renderAccount();
  try {
    await act(async () => {
      await view
        .player()
        .playQueue([recording], 0, { kind: "saved", position: 30 });
    });
    const audio = media.current();
    const observe = async (clock: number, position: number) => {
      await act(async () => {
        now = clock;
        audio.currentTime = position;
        audio.dispatchEvent(new Event("timeupdate"));
      });
    };
    act(() => {
      media.ready(audio);
      audio.dispatchEvent(new Event("seeked"));
      audio.dispatchEvent(new Event("playing"));
    });
    expect(audio.currentTime).toBe(30);
    expect(claims).toHaveLength(0);
    await observe(1000, 90);
    act(() => {
      view.player().seek(20);
      audio.dispatchEvent(new Event("seeked"));
    });
    await observe(11_000, 20);
    await act(async () => {
      await view.player().setPlaying(false);
    });
    await observe(16_000, 22);
    await act(async () => {
      await view.player().setPlaying(true);
    });
    act(() => audio.dispatchEvent(new Event("playing")));
    act(() => audio.load());
    await observe(30_000, 33);
    act(() => {
      media.ready(audio);
      audio.dispatchEvent(new Event("playing"));
    });
    await observe(Number.NaN, 35);
    await observe(40_000, 36);
    await observe(39_000, 37);
    await observe(40_000, 38);
    expect(claims).toHaveLength(0);
    for (let index = 1; index <= 4; index++)
      await observe(40_000 + index * 1000, 38 + index);
    expect(claims).toHaveLength(0);
    await observe(45_000, 43);
    expect(claims).toHaveLength(1);
    expect(claims[0].path).toBe(
      `/api/v1/sources/49858203-ce08-478f-95fe-482f34e376fb/successful-playback`,
    );
    expect(claims[0].options).toMatchObject({
      method: "POST",
      credentials: "same-origin",
      headers: {
        "x-hito-account": "owned-system-account",
        "x-hito-request": "1",
      },
      signal: expect.any(AbortSignal),
    });
    expect(JSON.parse(String(claims[0].options.body))).toEqual({
      submission_id: expect.stringMatching(
        /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
      ),
    });
    for (let index = 1; index <= 8; index++)
      await observe(45_000 + index * 1000, 43 + index);
    expect(claims).toHaveLength(1);
    expect(view.player().snapshot()).toMatchObject({
      playing: true,
      error: "",
      current: { id: recording.id },
    });
    expect(view.player().queueSnapshot().total).toBe(1);
    expect(view.renders()).toBe(1);
  } finally {
    view.unmount();
  }
});

test("source changes restart qualification and invalid oversized receipts cannot disturb playback", async () => {
  let now = 0;
  vi.spyOn(performance, "now").mockImplementation(() => now);
  const media = audiobookContinuationMedia();
  const claims: { path: string; body: string }[] = [];
  playbackRequests([recording, otherRecording], (path, options) => {
    if (!path.endsWith("/successful-playback")) return;
    claims.push({ path, body: String(options?.body) });
    return new Response("x".repeat(2048), {
      headers: { "content-type": "application/json" },
    });
  });
  const view = renderAccount();
  const heard = async (seconds: number) => {
    const audio = media.current();
    act(() => {
      media.ready(audio);
      audio.dispatchEvent(new Event("seeked"));
      audio.dispatchEvent(new Event("playing"));
    });
    for (let index = 1; index <= seconds; index++) {
      await act(async () => {
        now += 1000;
        audio.currentTime = index;
        audio.dispatchEvent(new Event("timeupdate"));
      });
    }
  };
  try {
    await act(async () => {
      await view.player().playQueue([recording, otherRecording]);
    });
    await heard(4);
    const prepared = media.prepared(otherRecording.id);
    expect(prepared).toBeDefined();
    act(() => media.ready(prepared!));
    await act(async () => {
      await view.player().next();
    });
    await heard(4);
    expect(claims).toHaveLength(0);
    const audio = media.current();
    act(() => {
      view.player().speed(2);
      audio.dispatchEvent(new Event("ratechange"));
      audio.dispatchEvent(new Event("timeupdate"));
    });
    await act(async () => {
      now += 500;
      audio.currentTime = 5;
      audio.dispatchEvent(new Event("timeupdate"));
    });
    expect(claims).toHaveLength(1);
    expect(claims[0].path).toBe(
      `/api/v1/sources/${otherRecording.id}/successful-playback`,
    );
    expect(view.player().snapshot()).toMatchObject({
      playing: true,
      error: "",
      current: { id: otherRecording.id },
    });
    expect(view.player().queueSnapshot().total).toBe(2);
  } finally {
    view.unmount();
  }
});

test("held community delivery has a bounded queue and never becomes a playback or progress tail", async () => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  let now = 0;
  vi.spyOn(performance, "now").mockImplementation(() => now);
  const media = audiobookContinuationMedia();
  const claims: string[] = [];
  let release!: ReadableStreamDefaultController<Uint8Array>;
  playbackRequests([recording], (path, options) => {
    if (!path.endsWith("/successful-playback")) return;
    claims.push(String(options?.body));
    return claims.length === 1
      ? new Response(
          new ReadableStream<Uint8Array>({
            start(controller) {
              release = controller;
            },
          }),
          { headers: { "content-type": "application/json" } },
        )
      : Response.json("queued");
  });
  const view = renderAccount();
  try {
    for (let episode = 0; episode < 26; episode++) {
      await act(async () => {
        await view.player().playQueue([recording]);
      });
      const audio = media.current();
      act(() => {
        media.ready(audio);
        media.observePlaying(audio);
      });
      for (let second = 1; second <= 5; second++) {
        await act(async () => {
          now += 1000;
          audio.currentTime = second;
          audio.dispatchEvent(new Event("timeupdate"));
        });
      }
    }
    expect(claims).toHaveLength(1);
    expect(view.player().snapshot()).toMatchObject({
      playing: true,
      error: "",
    });
    await act(async () => {
      await view.player().next();
    });
    expect(view.player().snapshot().current).toBeNull();
    await act(async () => {
      release.enqueue(new TextEncoder().encode(JSON.stringify("queued")));
      release.close();
    });
    expect(claims).toHaveLength(8);
    expect(
      new Set(claims.map((body) => JSON.parse(body).submission_id)).size,
    ).toBe(8);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(20_000);
    });
    expect(claims).toHaveLength(8);
  } finally {
    view.unmount();
  }
});

test("Continue waits for its private progress prefix while held community delivery stays independent and the fresh chapter qualifies anew", async () => {
  let now = 0;
  vi.spyOn(performance, "now").mockImplementation(() => now);
  const media = audiobookContinuationMedia();
  const { detail } = audiobookPages();
  const entry = detail.detail.chapters[1];
  const chapter = {
    audiobook_id: detail.detail.playlist.id,
    entry_id: entry.entry_id,
    recording_id: entry.recording_id!,
  };
  const fresh: components["schemas"]["AudiobookPlayback"] = {
    audiobook_id: chapter.audiobook_id,
    chapter,
    chapters: [entry],
    progress: {
      recording_id: chapter.recording_id,
      position_ms: 30000,
      speed: 1,
    },
  };
  const claims: string[] = [];
  let releaseCommunity!: ReadableStreamDefaultController<Uint8Array>;
  let releasePrefix!: () => void;
  let prefixSettled = false;
  let reads = 0;
  vi.stubGlobal(
    "fetch",
    vi.fn<typeof fetch>(async (input, options) => {
      const path = String(input);
      if (path === "/api/v1/sync") {
        const batch: components["schemas"]["SyncBatch"] = JSON.parse(
          String(options?.body),
        );
        if (!releasePrefix)
          await new Promise<void>((resolve) => {
            releasePrefix = () => {
              prefixSettled = true;
              resolve();
            };
          });
        return Response.json({
          acknowledged: batch.operations.map(({ id }) => id),
        });
      }
      if (path === `/api/v1/audiobooks/${chapter.audiobook_id}/playback`) {
        expect(prefixSettled).toBe(true);
        reads++;
        return Response.json(fresh);
      }
      if (path.endsWith("/successful-playback")) {
        claims.push(path);
        if (claims.length === 1)
          return new Response(
            new ReadableStream<Uint8Array>({
              start(controller) {
                releaseCommunity = controller;
              },
            }),
            { headers: { "content-type": "application/json" } },
          );
        return Response.json("queued");
      }
      const old = path === `/api/v1/recordings/${recording.id}/sources`;
      if (!old && path !== `/api/v1/recordings/${chapter.recording_id}/sources`)
        throw new Error("Unexpected owned Continue/community request");
      return Response.json([
        {
          id: old
            ? "49858203-ce08-478f-95fe-482f34e376fb"
            : "189b7475-24a0-4863-bb66-1492067a3fa3",
          recording_id: old ? recording.id : chapter.recording_id,
          provider: "plugin",
          device_id: null,
          ready: true,
          can_prepare: false,
          label: "Owned source",
        } satisfies components["schemas"]["Source"],
      ]);
    }),
  );
  const view = renderAccount();
  const scope = new AbortController();
  try {
    await act(async () => {
      await view.player().playQueue([recording]);
    });
    const old = media.current();
    act(() => {
      media.ready(old);
      media.observePlaying(old);
    });
    for (let second = 1; second <= 4; second++)
      await act(async () => {
        now += 1000;
        old.currentTime = second;
        old.dispatchEvent(new Event("timeupdate"));
      });
    let continuing!: Promise<boolean>;
    await act(async () => {
      continuing = view
        .player()
        .continueAudiobook(chapter.audiobook_id, scope.signal);
    });
    expect(reads).toBe(0);
    await act(async () => {
      now += 1000;
      old.currentTime = 5;
      old.dispatchEvent(new Event("timeupdate"));
    });
    expect(claims).toHaveLength(1);
    expect(old.paused).toBe(false);
    await act(async () => {
      releasePrefix();
      expect(await continuing).toBe(true);
    });
    expect(reads).toBe(1);
    expect(view.player().snapshot()).toMatchObject({
      current: { id: chapter.recording_id },
      error: "",
    });
    const current = media.current();
    await act(async () => {
      media.ready(current);
      current.dispatchEvent(new Event("seeked"));
      current.dispatchEvent(new Event("playing"));
      current.dispatchEvent(new Event("timeupdate"));
    });
    expect(current.currentTime).toBe(30);
    await act(async () => {
      releaseCommunity.enqueue(
        new TextEncoder().encode(JSON.stringify("queued")),
      );
      releaseCommunity.close();
    });
    for (let second = 1; second <= 4; second++)
      await act(async () => {
        now += 1000;
        current.currentTime = 30 + second;
        current.dispatchEvent(new Event("timeupdate"));
      });
    expect(claims).toHaveLength(1);
    await act(async () => {
      expect(
        await view
          .player()
          .continueAudiobook(chapter.audiobook_id, scope.signal),
      ).toBe(true);
    });
    expect(reads).toBe(2);
    const restarted = media.current();
    await act(async () => {
      media.ready(restarted);
      restarted.dispatchEvent(new Event("seeked"));
      restarted.dispatchEvent(new Event("playing"));
      restarted.dispatchEvent(new Event("timeupdate"));
    });
    expect(restarted.currentTime).toBe(30);
    for (let second = 1; second <= 4; second++)
      await act(async () => {
        now += 1000;
        restarted.currentTime = 30 + second;
        restarted.dispatchEvent(new Event("timeupdate"));
      });
    expect(claims).toHaveLength(1);
    await act(async () => {
      now += 1000;
      restarted.currentTime = 35;
      restarted.dispatchEvent(new Event("timeupdate"));
    });
    expect(claims).toEqual([
      "/api/v1/sources/49858203-ce08-478f-95fe-482f34e376fb/successful-playback",
      "/api/v1/sources/189b7475-24a0-4863-bb66-1492067a3fa3/successful-playback",
    ]);
    expect(view.player().snapshot()).toMatchObject({
      playing: true,
      error: "",
    });
  } finally {
    releasePrefix?.();
    scope.abort();
    view.unmount();
  }
});
