import { expect, test, vi } from "vitest";
import { Playback, type MediaPort } from "../playback";

class Media extends EventTarget implements MediaPort {
  src = "";
  private position = 0;
  get currentTime() {
    return this.position;
  }
  set currentTime(value: number) {
    this.position = value;
  }
  duration = 90;
  playbackRate = 1;
  defaultPlaybackRate = 1;
  volume = 1;
  paused = true;
  async play() {
    this.paused = false;
    this.dispatchEvent(new Event("play"));
  }
  pause() {
    this.paused = true;
    this.dispatchEvent(new Event("pause"));
  }
  load() {
    this.currentTime = 0;
  }
  removeAttribute() {
    this.src = "";
  }
}

const track = (id: string) => ({
  id,
  title: id,
  artist: "Fixture",
  duration_ms: 90000,
  available: true,
});

test("exhausted cached audio refreshes once and resumes the same recording near its saved position", async () => {
  const media = new Media();
  const requests: string[] = [];
  const player = new Playback(media, async (_id, options) => {
    requests.push(options.purpose);
    return [
      {
        id: options.purpose === "refresh" ? "replacement" : "expired",
        ready: true,
      },
    ];
  });
  await player.playQueue([track("saved")]);
  media.dispatchEvent(new Event("loadedmetadata"));
  player.seek(12.5);
  player.speed(1.25);
  await player.recover();
  expect(requests).toEqual(["cached", "refresh"]);
  expect(media.src).toContain("/replacement/audio");
  media.dispatchEvent(new Event("loadedmetadata"));
  expect(media.currentTime).toBe(12.5);
  expect(media.playbackRate).toBe(1.25);
  expect(player.snapshot().current?.id).toBe("saved");
  expect(player.snapshot().playing).toBe(true);
});

test("a rejected old refresh is aborted and cannot skip a newer queue selection", async () => {
  const media = new Media();
  let fail!: (error: Error) => void;
  let started!: () => void;
  let signal!: AbortSignal;
  const entered = new Promise<void>((resolve) => {
    started = resolve;
  });
  const player = new Playback(media, async (id, options) => {
    if (id === "old" && options.purpose === "refresh") {
      signal = options.signal;
      return new Promise((_, reject) => {
        fail = reject;
        started();
      });
    }
    return [{ id, ready: true }];
  });
  await player.playQueue([track("old")]);
  const recovery = player.recover();
  await entered;
  await player.playQueue([track("new"), track("following")]);
  expect(signal.aborted).toBe(true);
  fail(new Error("Private old refresh failed"));
  await recovery;
  expect(player.snapshot().current?.id).toBe("new");
  expect(player.snapshot().error).toBe("");
  expect(media.src).toContain("/new/audio");
});

test("a seek made while replacement audio is being found wins over the failed position", async () => {
  const media = new Media();
  let finish!: (sources: { id: string; ready: boolean }[]) => void;
  let started!: () => void;
  const entered = new Promise<void>((resolve) => {
    started = resolve;
  });
  const player = new Playback(media, async (_id, options) =>
    options.purpose === "refresh"
      ? new Promise((resolve) => {
          finish = resolve;
          started();
        })
      : [{ id: "expired", ready: true }],
  );
  await player.playQueue([track("saved")]);
  media.dispatchEvent(new Event("loadedmetadata"));
  player.seek(12.5);
  const recovery = player.recover();
  await entered;
  player.seek(30);
  finish([{ id: "replacement", ready: true }]);
  await recovery;
  media.dispatchEvent(new Event("loadedmetadata"));
  expect(media.currentTime).toBe(30);
  expect(player.snapshot().position).toBe(30);
});

test("cached alternatives play before a provider refresh and every source cycle is bounded", async () => {
  class FailingMedia extends Media {
    attempts: string[] = [];
    override async play() {
      this.attempts.push(this.src);
      if (!this.src.includes("/healthy/")) throw new Error("Audio failed");
      await super.play();
    }
  }
  const media = new FailingMedia();
  const requests: string[] = [];
  const player = new Playback(media, async (id, options) => {
    requests.push(`${id}:${options.purpose}`);
    if (id === "next") return [{ id: "healthy", ready: true }];
    return [
      { id: "failed-a", ready: true },
      { id: "failed-b", ready: true },
      { id: "not-ready", ready: false },
    ];
  });
  await player.playQueue([track("failed"), track("next")]);
  expect(requests).toEqual([
    "failed:cached",
    "failed:refresh",
    "failed:resolve",
    "next:cached",
  ]);
  expect(media.attempts.map((path) => path.split("/").at(-2))).toEqual([
    "failed-a",
    "failed-b",
    "failed-a",
    "failed-b",
    "healthy",
  ]);
  expect(player.snapshot().current?.id).toBe("next");
  expect(player.snapshot().playing).toBe(true);
  expect(player.snapshot().error).toContain("Audio is unavailable for failed");
});

test("pausing while a refresh is pending stops the search, keeps the track selected and ignores the late replacement", async () => {
  const media = new Media();
  let finish!: (sources: { id: string; ready: boolean }[]) => void;
  let started!: () => void;
  let signal!: AbortSignal;
  const entered = new Promise<void>((resolve) => {
    started = resolve;
  });
  const player = new Playback(media, async (_id, options) => {
    if (options.purpose !== "refresh") return [{ id: "expired", ready: true }];
    signal = options.signal;
    return new Promise((resolve) => {
      finish = resolve;
      started();
    });
  });
  await player.playQueue([track("saved")]);
  const recovery = player.recover();
  await entered;
  expect(player.snapshot().desiredPlaying).toBe(true);
  await player.toggle();
  expect(signal.aborted).toBe(true);
  expect(media.paused).toBe(true);
  expect(player.snapshot().desiredPlaying).toBe(false);
  expect(player.snapshot().loading).toBe(false);
  expect(player.snapshot().current?.id).toBe("saved");
  finish([{ id: "replacement", ready: true }]);
  await recovery;
  expect(media.src).toBe("");
  expect(player.snapshot().playing).toBe(false);
  expect(player.snapshot().error).toBe("");
  expect(player.snapshot().retrying ?? null).toBeNull();
  expect(player.snapshot().current?.id).toBe("saved");
});

test("pressing Play while a refresh is pending neither pauses nor repeats the search", async () => {
  const media = new Media();
  let finish!: (sources: { id: string; ready: boolean }[]) => void;
  let started!: () => void;
  const purposes: string[] = [];
  const entered = new Promise<void>((resolve) => {
    started = resolve;
  });
  const player = new Playback(media, async (_id, options) => {
    purposes.push(options.purpose);
    return options.purpose === "refresh"
      ? new Promise((resolve) => {
          finish = resolve;
          started();
        })
      : [{ id: "expired", ready: true }];
  });
  await player.playQueue([track("saved")]);
  const recovery = player.recover();
  await entered;
  await player.setPlaying(true);
  expect(purposes).toEqual(["cached", "refresh"]);
  finish([{ id: "replacement", ready: true }]);
  await recovery;
  expect(media.src).toContain("/replacement/audio");
  expect(player.snapshot().playing).toBe(true);
  expect(purposes).toEqual(["cached", "refresh"]);
});

test("a failed refresh can use a newer cached source accepted by another request", async () => {
  const media = new Media();
  let reads = 0;
  const calls: string[] = [];
  const player = new Playback(media, async (_id, options) => {
    calls.push(options.purpose);
    if (options.purpose === "refresh")
      throw new Error("Sources changed during refresh");
    return [{ id: reads++ === 0 ? "expired" : "peer-source", ready: true }];
  });
  await player.playQueue([track("saved")]);
  await player.recover();
  expect(calls).toEqual(["cached", "refresh", "cached"]);
  expect(media.src).toContain("/peer-source/audio");
  expect(player.snapshot().error).toBe("");
});

test("a failed seek recovers through another source at the requested position", async () => {
  class FailedSeekMedia extends Media {
    override get currentTime() {
      return super.currentTime;
    }
    override set currentTime(value: number) {
      if (this.src.includes("/expired/") && value > 0)
        throw new DOMException("Seek failed", "InvalidStateError");
      super.currentTime = value;
    }
  }
  const media = new FailedSeekMedia();
  const player = new Playback(media, async () => [
    { id: "expired", ready: true },
    { id: "alternative", ready: true },
  ]);
  await player.playQueue([track("saved")]);
  media.dispatchEvent(new Event("loadedmetadata"));
  expect(() => player.seek(12.5)).not.toThrow();
  await vi.waitFor(() => expect(media.src).toContain("/alternative/audio"));
  media.dispatchEvent(new Event("loadedmetadata"));
  expect(media.currentTime).toBe(12.5);
  expect(player.snapshot().current?.id).toBe("saved");
});

test.each([10, 0])(
  "a failed backward seek to %s retains the requested position",
  async (target) => {
    class FailedSeekMedia extends Media {
      failSeek = false;
      override get currentTime() {
        return super.currentTime;
      }
      override set currentTime(value: number) {
        if (this.failSeek && this.src.includes("/expired/"))
          throw new DOMException("Seek failed", "InvalidStateError");
        super.currentTime = value;
      }
    }
    const media = new FailedSeekMedia();
    const player = new Playback(media, async () => [
      { id: "expired", ready: true },
      { id: "alternative", ready: true },
    ]);
    await player.playQueue([track("saved")]);
    media.dispatchEvent(new Event("loadedmetadata"));
    player.seek(40);
    media.failSeek = true;
    player.seek(target);
    await vi.waitFor(() => expect(media.src).toContain("/alternative/audio"));
    media.dispatchEvent(new Event("loadedmetadata"));
    expect(media.currentTime).toBe(target);
    expect(player.snapshot().position).toBe(target);
  },
);
