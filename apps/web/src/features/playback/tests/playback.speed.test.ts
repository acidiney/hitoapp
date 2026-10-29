import { expect, test } from "vitest";
import { Playback, type MediaPort } from "../playback";

class Media extends EventTarget implements MediaPort {
  src = "";
  currentTime = 0;
  duration = 10;
  defaultPlaybackRate = 1;
  volume = 1;
  paused = true;
  rate = 1;
  limited = (src: string) => src.includes("limited");
  refusal: "clamp" | "throw" = "clamp";
  get playbackRate() {
    return this.rate;
  }
  set playbackRate(value: number) {
    if (this.limited(this.src) && value !== 1) {
      if (this.refusal === "throw")
        throw new DOMException("Unsupported rate", "NotSupportedError");
      return;
    }
    this.rate = value;
  }
  async play() {
    this.paused = false;
    this.dispatchEvent(new Event("play"));
  }
  pause() {
    this.paused = true;
    this.dispatchEvent(new Event("pause"));
  }
  load() {
    this.rate = this.limited(this.src) ? 1 : this.defaultPlaybackRate;
  }
  removeAttribute() {
    this.src = "";
  }
}

const track = (id: string) => ({
  id,
  title: id,
  artist: "Test",
  duration_ms: 10000,
  available: true,
});

function start(refusal: "clamp" | "throw") {
  const media = new Media();
  media.refusal = refusal;
  const player = new Playback(media, async (id) => [{ id, ready: true }]);
  return { media, player };
}

test("a browser source accepts the chosen speed and reports the full range", async () => {
  const { media, player } = start("clamp");
  player.speed(1.5);
  await player.playQueue([track("plain")]);
  media.dispatchEvent(new Event("loadedmetadata"));
  expect(media.playbackRate).toBe(1.5);
  expect(player.snapshot()).toMatchObject({
    speed: 1.5,
    effectiveSpeed: 1.5,
    speedRange: { min: 0.5, max: 3 },
  });
});

test.each(["clamp", "throw"] as const)(
  "a browser source that %ss the rate plays at 1× and the chosen speed returns with the next source",
  async (refusal) => {
    const { media, player } = start(refusal);
    player.speed(1.5);
    await player.playQueue([
      track("limited-a"),
      track("plain"),
      track("limited-b"),
    ]);
    media.dispatchEvent(new Event("loadedmetadata"));
    expect(player.snapshot()).toMatchObject({
      speed: 1.5,
      effectiveSpeed: 1,
      speedRange: { min: 1, max: 1 },
    });
    expect(media.playbackRate).toBe(1);
    await player.next();
    media.dispatchEvent(new Event("loadedmetadata"));
    expect(player.snapshot()).toMatchObject({
      speed: 1.5,
      effectiveSpeed: 1.5,
      speedRange: { min: 0.5, max: 3 },
    });
    expect(media.playbackRate).toBe(1.5);
    await player.next();
    media.dispatchEvent(new Event("loadedmetadata"));
    expect(player.snapshot()).toMatchObject({
      speed: 1.5,
      effectiveSpeed: 1,
      speedRange: { min: 1, max: 1 },
    });
  },
);

test("choosing a speed on a source that refuses it keeps the choice and reports 1×", async () => {
  const { media, player } = start("clamp");
  await player.playQueue([track("limited")]);
  media.dispatchEvent(new Event("loadedmetadata"));
  player.speed(2);
  expect(player.snapshot()).toMatchObject({
    speed: 2,
    effectiveSpeed: 1,
    speedRange: { min: 1, max: 1 },
  });
  expect(media.playbackRate).toBe(1);
});
