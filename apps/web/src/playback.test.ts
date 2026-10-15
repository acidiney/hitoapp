import { expect, test } from "vitest";
import { Playback, type MediaPort } from "./playback";

class Media extends EventTarget implements MediaPort {
  src = "";
  currentTime = 0;
  duration = 10;
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
  load() {}
  removeAttribute() {
    this.src = "";
  }
}

test("editing and resetting a queued title preserves playback and each release fallback", async () => {
  const audio = new Media();
  let resolutions = 0;
  const player = new Playback(audio, async () => {
    resolutions++;
    return [{ id: "source", ready: true }];
  });
  await player.playQueue([
    {
      id: "same",
      title: "Original title",
      inherited_title: "Original title",
      artist: "Test",
      duration_ms: 10000,
      available: true,
    },
    {
      id: "same",
      title: "Encore",
      inherited_title: "Encore",
      artist: "Test",
      duration_ms: 10000,
      available: true,
    },
  ]);
  player.seek(4);
  const source = audio.src;
  player.setRecordingField("same", "title", "Personal title", "Original title");
  expect(player.snapshot().queue.map((track) => track.title)).toEqual([
    "Personal title",
    "Personal title",
  ]);
  expect(player.snapshot().current?.title).toBe("Personal title");
  player.setRecordingField("same", "title", null, "Original title");
  expect(player.snapshot().queue.map((track) => track.title)).toEqual([
    "Original title",
    "Encore",
  ]);
  expect(player.snapshot().current?.title).toBe("Original title");
  expect(audio.src).toBe(source);
  expect(audio.currentTime).toBe(4);
  expect(audio.paused).toBe(false);
  expect(resolutions).toBe(1);
});

test("a failed source recovers the same recording near the current position", async () => {
  const audio = new Media();
  const player = new Playback(audio, async () => [
    { id: "first", ready: true },
    { id: "second", ready: true },
  ]);
  await player.playQueue(
    [
      {
        id: "recording",
        title: "Fixture",
        artist: "Test",
        duration_ms: 10000,
        available: true,
      },
    ],
    0,
  );
  expect(audio.src).toContain("first");
  audio.currentTime = 4.5;
  await player.recover();
  expect(audio.src).toContain("second");
  audio.dispatchEvent(new Event("loadedmetadata"));
  expect(audio.currentTime).toBe(4.5);
  expect(player.snapshot().current?.id).toBe("recording");
});

test("performer edits and resets preserve audio and each appearance's credit", async () => {
  const media = new Media();
  const player = new Playback(media, async () => [
    { id: "source", ready: true },
  ]);
  const track = {
    id: "recording",
    title: "Title",
    duration_ms: 10000,
    available: true,
  };
  await player.playQueue([
    { ...track, artist: "Original", inherited_artist: "Original" },
    { ...track, artist: "Guest", inherited_artist: "Guest" },
    { ...track, id: "other", artist: "Unchanged" },
  ]);
  player.seek(3);
  player.speed(1.25);
  const source = media.src;
  player.setRecordingField("recording", "artist", "Personal", "Original");
  expect(player.snapshot().queue.map((item) => item.artist)).toEqual([
    "Personal",
    "Personal",
    "Unchanged",
  ]);
  expect(player.snapshot().current?.artist).toBe("Personal");
  player.setRecordingField("recording", "artist", null, "Original");
  expect(player.snapshot().queue.map((item) => item.artist)).toEqual([
    "Original",
    "Guest",
    "Unchanged",
  ]);
  expect(player.snapshot().current?.artist).toBe("Original");
  expect(media.src).toBe(source);
  expect(media.currentTime).toBe(3);
  expect(media.playbackRate).toBe(1.25);
  expect(media.paused).toBe(false);
});

test("an old media error while resolving cannot replace the selected track", async () => {
  const audio = new Media();
  let resolve!: (sources: { id: string; ready: boolean }[]) => void;
  const player = new Playback(
    audio,
    () =>
      new Promise((done) => {
        resolve = done;
      }),
  );
  const opening = player.playQueue(
    [
      {
        id: "selected",
        title: "Selected",
        artist: "Test",
        duration_ms: 10000,
        available: true,
      },
    ],
    0,
  );
  audio.dispatchEvent(new Event("error"));
  resolve([{ id: "selected-source", ready: true }]);
  await opening;
  expect(player.snapshot().current?.id).toBe("selected");
  expect(player.snapshot().error).toBe("");
  expect(audio.src).toContain("selected-source");
});

test("a rejected play promise from an earlier selection cannot skip the new track", async () => {
  let rejectOld!: (error: Error) => void;
  class DelayedMedia extends Media {
    calls = 0;
    override async play() {
      if (this.calls++ === 0)
        await new Promise<void>((_, reject) => {
          rejectOld = reject;
        });
      else await super.play();
    }
  }
  const audio = new DelayedMedia();
  const track = (id: string) => ({
    id,
    title: id,
    artist: "Test",
    duration_ms: 10000,
    available: true,
  });
  const player = new Playback(audio, async (id) => [{ id, ready: true }]);
  const old = player.playQueue([track("old")]);
  await Promise.resolve();
  await player.playQueue([track("new"), track("next")]);
  const aborted = new Error("aborted");
  aborted.name = "AbortError";
  rejectOld(aborted);
  await old;
  expect(player.snapshot().current?.id).toBe("new");
  expect(audio.src).toContain("/new/");
  expect(player.snapshot().error).toBe("");
});

test("natural completion saves the outgoing recording before resetting media", async () => {
  class ResetMedia extends Media {
    override load() {
      this.currentTime = 0;
    }
  }
  const saved: { id: string; position: number }[] = [];
  const media = new ResetMedia();
  const player = new Playback(
    media,
    async (id) => [{ id, ready: true }],
    (track, position) => {
      saved.push({ id: track.id, position });
    },
  );
  await player.playQueue([
    {
      id: "short",
      title: "Short",
      artist: "Test",
      duration_ms: 1000,
      available: true,
    },
  ]);
  media.currentTime = 1;
  media.dispatchEvent(new Event("ended"));
  expect(saved).toContainEqual({ id: "short", position: 1 });
});

test.each(["selection", "recovery", "clear"])(
  "a delayed resume rejection cannot change playback after %s",
  async (transition) => {
    let rejectResume: ((error: Error) => void) | undefined;
    class DelayedResumeMedia extends Media {
      delayNext = false;
      override async play() {
        if (this.delayNext) {
          this.delayNext = false;
          await new Promise<void>((_, reject) => {
            rejectResume = reject;
          });
        } else await super.play();
      }
    }
    const audio = new DelayedResumeMedia();
    const track = (id: string) => ({
      id,
      title: id,
      artist: "Test",
      duration_ms: 10000,
      available: true,
    });
    const player = new Playback(audio, async (id) => [
      { id: `${id}-first`, ready: true },
      { id: `${id}-second`, ready: true },
    ]);
    await player.playQueue([track("old")]);
    await player.toggle();
    audio.delayNext = true;
    const resume = player.toggle();
    if (transition === "selection") await player.playQueue([track("new")]);
    else if (transition === "recovery") await player.recover();
    else player.clear();
    if (!rejectResume) throw new Error("Resume did not wait");
    const aborted = new Error("Previous resume was aborted");
    aborted.name = "AbortError";
    rejectResume(aborted);
    await resume;
    expect(player.snapshot().error).toBe("");
    expect(player.snapshot().current?.id).toBe(
      transition === "selection"
        ? "new"
        : transition === "recovery"
          ? "old"
          : undefined,
    );
    expect(player.snapshot().playing).toBe(transition !== "clear");
  },
);

test("the selected speed survives source loading, recovery, queue transitions, and clearing", async () => {
  class ReloadingMedia extends Media {
    override load() {
      this.playbackRate = this.defaultPlaybackRate;
    }
  }
  const media = new ReloadingMedia();
  const player = new Playback(media, async (id) => [
    { id: `${id}-first`, ready: true },
    { id: `${id}-second`, ready: true },
  ]);
  const track = (id: string) => ({
    id,
    title: id,
    artist: "Test",
    duration_ms: 10000,
    available: true,
  });
  player.speed(1.5);
  await player.playQueue([track("first"), track("next")]);
  expect(media.playbackRate).toBe(1.5);
  await player.recover();
  expect(media.playbackRate).toBe(1.5);
  await player.next();
  expect(media.playbackRate).toBe(1.5);
  player.clear();
  expect(media.playbackRate).toBe(1.5);
});
