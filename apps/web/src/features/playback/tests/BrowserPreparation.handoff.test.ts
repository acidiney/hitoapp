import { setImmediate as flushTasks } from "node:timers/promises";
import { afterEach, expect, test } from "vitest";
import {
  audioPath,
  browserPreparationModes,
  recording,
} from "../fixtures/browserPreparationModes";

afterEach(() => {
  Reflect.deleteProperty(document, "visibilityState");
});

test("natural completion plays the buffered element without loading its audio again and keeps volume and speed", async () => {
  const fixture = browserPreparationModes();
  const { player, initial } = fixture;

  try {
    await player.playQueue([recording("first"), recording("next")]);
    player.volume(0.4);
    player.speed(1.5);
    const prepared = await fixture.buffer("next-audio");
    prepared.ready();

    initial.complete();
    expect(prepared.played).toEqual([audioPath("next-audio")]);

    expect(fixture.playing()).toBe(prepared);
    expect(prepared.loads).toEqual([audioPath("next-audio")]);
    expect(initial.loads).not.toContain(audioPath("next-audio"));
    expect(initial.played).toEqual([audioPath("first-audio")]);
    expect(fixture.released).toContain(initial);
    expect(prepared.volume).toBe(0.4);
    expect(prepared.playbackRate).toBe(1.5);
    expect(prepared.defaultPlaybackRate).toBe(1.5);
    expect(player.snapshot()).toMatchObject({
      current: { id: "next" },
      playing: true,
      loading: false,
      error: "",
    });
    expect(fixture.requests.map(({ recording }) => recording)).toEqual([
      "first",
      "next",
    ]);

    prepared.currentTime = 5;
    prepared.dispatchEvent(new Event("timeupdate"));
    expect(player.positionSnapshot().position).toBe(5);
    initial.currentTime = 40;
    for (const event of ["timeupdate", "ended", "error", "pause"])
      initial.dispatchEvent(new Event(event));
    expect(player.positionSnapshot().position).toBe(5);
    expect(player.snapshot().current?.id).toBe("next");
    expect(prepared.played).toHaveLength(1);
  } finally {
    await fixture.close();
  }
});

test("a successor that changes after preparation plays from its own buffer and the stale buffer never plays", async () => {
  const fixture = browserPreparationModes();
  const { player, initial } = fixture;

  try {
    await player.playQueue([
      recording("first"),
      recording("planned"),
      recording("chosen"),
    ]);
    const entries = (await player.queuePage(0, 10)).entries;
    const planned = await fixture.buffer("planned-audio");
    planned.ready();

    player.moveEntry(entries[2].id, entries[1].id);
    expect(fixture.released).toContain(planned);
    const chosen = await fixture.buffer("chosen-audio");
    chosen.ready();
    initial.complete();

    expect(fixture.playing()).toBe(chosen);
    expect(chosen.played).toEqual([audioPath("chosen-audio")]);
    expect(chosen.loads).toEqual([audioPath("chosen-audio")]);
    expect(planned.played).toEqual([]);
    expect(initial.loads).not.toContain(audioPath("chosen-audio"));
    expect(player.snapshot().current?.id).toBe("chosen");
    expect(player.queueSnapshot().currentEntry).toBe(entries[2].id);
  } finally {
    await fixture.close();
  }
});

test("seeking the current track keeps the buffer for the boundary and later seeks move the element that now plays", async () => {
  const fixture = browserPreparationModes();
  const { player, initial } = fixture;

  try {
    await player.playQueue([recording("first"), recording("next")]);
    initial.ready();
    const prepared = await fixture.buffer("next-audio");
    prepared.ready();

    player.seek(80);
    expect(initial.currentTime).toBe(80);
    expect(prepared.currentTime).toBe(0);
    expect(fixture.owned.has(prepared)).toBe(true);
    initial.complete();

    expect(fixture.playing()).toBe(prepared);
    expect(prepared.loads).toEqual([audioPath("next-audio")]);
    player.seek(30);
    expect(prepared.currentTime).toBe(30);
    expect(initial.currentTime).toBe(0);
    expect(player.positionSnapshot().position).toBe(30);
  } finally {
    await fixture.close();
  }
});

test("a stall while the next track buffers leaves current audio alone and a later hidden stall retries the element that now plays", async () => {
  Object.defineProperty(document, "visibilityState", {
    configurable: true,
    get: () => "hidden",
  });
  const fixture = browserPreparationModes();
  const { player, initial } = fixture;

  try {
    await player.playQueue([recording("first"), recording("next")]);
    const prepared = await fixture.buffer("next-audio");
    const loads = [...initial.loads];
    prepared.dispatchEvent(new Event("waiting"));
    prepared.dispatchEvent(new Event("stalled"));
    expect(initial.played).toEqual([audioPath("first-audio")]);
    expect(initial.loads).toEqual(loads);
    prepared.ready();
    initial.complete();

    expect(fixture.playing()).toBe(prepared);
    prepared.currentTime = 30;
    prepared.dispatchEvent(new Event("timeupdate"));
    prepared.readyState = 2;
    prepared.dispatchEvent(new Event("stalled"));

    expect(prepared.loads).toEqual([
      audioPath("next-audio"),
      audioPath("next-audio"),
    ]);
    expect(prepared.played).toEqual([
      audioPath("next-audio"),
      audioPath("next-audio"),
    ]);
    expect(initial.played).toEqual([audioPath("first-audio")]);
    prepared.ready();
    expect(prepared.currentTime).toBe(30);
  } finally {
    await fixture.close();
  }
});

test("a browser that refuses to start the buffered element plays the next source on the element that was already playing", async () => {
  const fixture = browserPreparationModes();
  const { player, initial } = fixture;

  try {
    await player.playQueue([recording("first"), recording("next")]);
    const prepared = await fixture.buffer("next-audio");
    prepared.ready();
    prepared.refusal = Object.assign(new Error("Playback needs a gesture"), {
      name: "NotAllowedError",
    });

    initial.complete();
    await flushTasks();

    expect(fixture.playing()).toBe(initial);
    expect(initial.played).toEqual([
      audioPath("first-audio"),
      audioPath("next-audio"),
    ]);
    expect(initial.src).toBe(audioPath("next-audio"));
    expect(fixture.released).toContain(prepared);
    expect(player.snapshot()).toMatchObject({
      current: { id: "next" },
      playing: true,
      error: "",
    });
  } finally {
    await fixture.close();
  }
});
