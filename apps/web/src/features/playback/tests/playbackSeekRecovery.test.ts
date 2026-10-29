import { expect, test } from "vitest";
import { Playback, type MediaPort } from "../playback";

class Media extends EventTarget implements MediaPort {
  src = "";
  private position = 0;
  get currentTime() {
    return this.position;
  }
  set currentTime(value: number) {
    if (this.readyState > 0) this.position = value;
    else if (value === 0) this.position = 0;
  }
  duration = 90;
  readyState = 4;
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
    this.position = 0;
    this.readyState = 0;
    this.duration = Number.NaN;
  }
  ready(duration = 90) {
    this.readyState = 1;
    this.duration = duration;
    this.dispatchEvent(new Event("loadedmetadata"));
  }
  removeAttribute() {
    this.src = "";
  }
}

const track = {
  id: "saved",
  title: "saved",
  artist: "Fixture",
  duration_ms: 90000,
  available: true,
};

test("the reported position keeps the requested seek while replacement audio loads", async () => {
  const media = new Media();
  const player = new Playback(media, async (_id, options) => [
    {
      id: options.purpose === "refresh" ? "replacement" : "expired",
      ready: true,
    },
  ]);
  await player.playQueue([track]);
  media.ready();
  player.seek(12.5);
  await player.recover();
  media.currentTime = 0;
  media.dispatchEvent(new Event("timeupdate"));
  expect(player.positionSnapshot().position).toBe(12.5);
  media.ready();
  expect(media.currentTime).toBe(12.5);
  expect(player.positionSnapshot().position).toBe(12.5);
});

async function erroredPlayer() {
  const media = new Media();
  const player = new Playback(media, async (_id, options) => [
    {
      id: options.purpose === "refresh" ? "replacement" : "expired",
      ready: true,
    },
  ]);
  await player.playQueue([track]);
  media.ready();
  player.seek(9.3);
  media.currentTime = 9.3;
  media.dispatchEvent(new Event("error"));
  await new Promise((resolve) => setTimeout(resolve, 0));
  return { media, player };
}

test("an unknown duration while replacement audio loads keeps the last known duration", async () => {
  const { media, player } = await erroredPlayer();
  media.dispatchEvent(new Event("durationchange"));
  expect(player.snapshot().duration).toBe(90);
});

test.each([18.3, 0.1])(
  "a seek to %f while replacement audio has no metadata is applied once metadata loads",
  async (target) => {
    const { media, player } = await erroredPlayer();
    expect(media.readyState).toBe(0);
    media.dispatchEvent(new Event("durationchange"));
    player.seek(target);
    expect(player.positionSnapshot().position).toBe(target);
    media.ready();
    expect(media.currentTime).toBe(target);
  },
);

test("recovery resumes from the last requested position when the failed element reports zero", async () => {
  const media = new Media();
  const player = new Playback(media, async (_id, options) => [
    {
      id: options.purpose === "refresh" ? "replacement" : "expired",
      ready: true,
    },
  ]);
  await player.playQueue([track]);
  media.ready();
  player.seek(9.3);
  media.currentTime = 0;
  media.dispatchEvent(new Event("error"));
  await new Promise((resolve) => setTimeout(resolve, 0));
  media.ready();
  expect(media.currentTime).toBe(9.3);
});
