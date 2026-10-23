import { expect, test } from "vitest";
import { Playback, type MediaPort } from "../playback";

class Media extends EventTarget implements MediaPort {
  src = "";
  currentTime = 0;
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
  media.dispatchEvent(new Event("loadedmetadata"));
  player.seek(12.5);
  await player.recover();
  media.currentTime = 0;
  media.dispatchEvent(new Event("timeupdate"));
  expect(player.positionSnapshot().position).toBe(12.5);
  media.dispatchEvent(new Event("loadedmetadata"));
  expect(media.currentTime).toBe(12.5);
  expect(player.positionSnapshot().position).toBe(12.5);
});
