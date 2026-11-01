import { expect, test } from "vitest";
import { Playback, type MediaPort } from "../playback";

const MEDIA_ERR_NETWORK = 2;
const MEDIA_ERR_SRC_NOT_SUPPORTED = 4;

class Media extends EventTarget implements MediaPort {
  src = "";
  currentTime = 0;
  duration = 10;
  playbackRate = 1;
  defaultPlaybackRate = 1;
  volume = 1;
  paused = true;
  error: { code: number } | null = null;
  rejection: Error | null = null;
  async play() {
    if (this.rejection) throw this.rejection;
    this.paused = false;
    this.dispatchEvent(new Event("play"));
  }
  pause() {
    this.paused = true;
    this.dispatchEvent(new Event("pause"));
  }
  load() {}
  removeAttribute() {
    this.src = "";
  }
}

const track = (id: string) => ({
  id,
  title: id,
  artist: "Fixture",
  duration_ms: 10000,
  available: true,
});

const UNSUPPORTED = "This device can't play this audio format.";

async function playing(sources: { id: string; ready: boolean }[]) {
  const media = new Media();
  const player = new Playback(media, async (_id, options) =>
    options.purpose === "cached" ? [{ id: "first", ready: true }] : sources,
  );
  await player.playQueue([track("first")]);
  return { media, player };
}

test("a source the element cannot decode reports an unsupported format once sources are exhausted", async () => {
  const { media, player } = await playing([]);
  media.error = { code: MEDIA_ERR_SRC_NOT_SUPPORTED };
  media.dispatchEvent(new Event("error"));
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(player.snapshot().error).toBe(UNSUPPORTED);
});

test("a play rejection for an unsupported source reports an unsupported format", async () => {
  const media = new Media();
  media.rejection = Object.assign(new Error("no"), {
    name: "NotSupportedError",
  });
  const player = new Playback(media, async (_id, options) =>
    options.purpose === "cached" ? [{ id: "first", ready: true }] : [],
  );
  await player.playQueue([track("first")]);
  expect(player.snapshot().error).toBe(UNSUPPORTED);
});

test("other media failures keep the generic unavailable message", async () => {
  const { media, player } = await playing([]);
  media.error = { code: MEDIA_ERR_NETWORK };
  media.dispatchEvent(new Event("error"));
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(player.snapshot().error).toBe("Audio is unavailable for first.");
});

test("an unsupported format does not carry over to the next track", async () => {
  const { media, player } = await playing([]);
  media.error = { code: MEDIA_ERR_SRC_NOT_SUPPORTED };
  media.dispatchEvent(new Event("error"));
  await new Promise((resolve) => setTimeout(resolve, 0));
  media.error = null;
  await player.playQueue([track("second")]);
  media.error = { code: MEDIA_ERR_NETWORK };
  media.dispatchEvent(new Event("error"));
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(player.snapshot().error).toBe("Audio is unavailable for second.");
});
