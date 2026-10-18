import { afterEach, expect, test, vi } from "vitest";
import {
  audioPath,
  browserPreparationModes,
  recording,
} from "./fixtures/browserPreparationModes";

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

test("shuffle cancels the canonical buffer and adopts the shuffled successor on natural completion", async () => {
  vi.spyOn(Math, "random").mockReturnValue(0);
  const fixture = browserPreparationModes();
  const { player, initial } = fixture;

  try {
    await player.playQueue([
      recording("first"),
      recording("canonical"),
      recording("shuffled"),
    ]);
    const entries = (await player.queuePage(0, 10)).entries;
    const canonical = await fixture.buffer("canonical-audio");
    const superseded = fixture.requests.filter(
      ({ recording }) => recording === "canonical",
    );
    expect(superseded).toHaveLength(1);

    player.shuffle();
    expect(superseded.every(({ signal }) => signal.aborted)).toBe(true);
    expect(canonical.src).toBe("");
    expect(fixture.owned.has(canonical)).toBe(false);
    canonical.ready();
    expect(canonical.played).toEqual([]);

    const prepared = await fixture.buffer("shuffled-audio");
    expect(prepared.src).toBe(audioPath("shuffled-audio"));
    expect(prepared.paused).toBe(true);
    expect(prepared.played).toEqual([]);
    expect(initial.src).toBe(audioPath("first-audio"));
    expect(initial.paused).toBe(false);
    prepared.ready();
    initial.complete();
    await prepared.playing;

    expect(fixture.activated).toEqual([prepared]);
    expect(prepared.loads).toEqual([audioPath("shuffled-audio")]);
    expect(prepared.played).toEqual([audioPath("shuffled-audio")]);
    expect(player.snapshot().current?.id).toBe("shuffled");
    expect(player.snapshot().playing).toBe(true);
    expect(player.queueSnapshot().currentEntry).toBe(entries[2].id);
    expect(player.queueSnapshot().currentEntry).not.toBe(entries[1].id);
    expect((await player.queuePage(0, 10)).entries).toEqual(entries);
    expect(fixture.requests.every(({ purpose }) => purpose === "cached")).toBe(
      true,
    );
  } finally {
    await fixture.close();
  }
});

test("repeat one prepares the current occurrence and adopts it when its audio naturally ends", async () => {
  const fixture = browserPreparationModes();
  const { player, initial } = fixture;

  try {
    player.repeat();
    player.repeat();
    await player.playQueue([recording("first"), recording("next")]);
    const entries = (await player.queuePage(0, 10)).entries;
    const prepared = await fixture.buffer("first-audio");
    expect(player.snapshot().repeat).toBe("one");
    expect(prepared.src).toBe(audioPath("first-audio"));
    expect(prepared.paused).toBe(true);
    expect(prepared.played).toEqual([]);
    prepared.ready();
    initial.complete();
    await prepared.playing;

    expect(fixture.activated).toEqual([prepared]);
    expect(prepared.loads).toEqual([audioPath("first-audio")]);
    expect(prepared.played).toEqual([audioPath("first-audio")]);
    expect(prepared.currentTime).toBe(0);
    expect(player.snapshot().current?.id).toBe("first");
    expect(player.snapshot().playing).toBe(true);
    expect(player.queueSnapshot().currentEntry).toBe(entries[0].id);
    expect((await player.queuePage(0, 10)).entries).toEqual(entries);
    expect(
      fixture.requests.map(
        ({ recording, purpose }) => `${recording}:${purpose}`,
      ),
    ).toEqual(["first:cached", "first:cached"]);
    const following = await fixture.buffer("first-audio");
    expect(following).not.toBe(prepared);
    expect(following.paused).toBe(true);
    expect(fixture.owned.size).toBe(2);
  } finally {
    await fixture.close();
  }
});

test("repeat all prepares the first occurrence and adopts it after the last audio naturally ends", async () => {
  const fixture = browserPreparationModes();
  const { player, initial } = fixture;

  try {
    player.repeat();
    await player.playQueue([recording("first"), recording("last")], 1);
    const entries = (await player.queuePage(0, 10)).entries;
    expect(player.queueSnapshot().currentEntry).toBe(entries[1].id);
    const prepared = await fixture.buffer("first-audio");
    expect(player.snapshot().repeat).toBe("all");
    expect(prepared.src).toBe(audioPath("first-audio"));
    expect(prepared.paused).toBe(true);
    expect(prepared.played).toEqual([]);
    expect(initial.src).toBe(audioPath("last-audio"));
    prepared.ready();
    initial.complete();
    await prepared.playing;

    expect(fixture.activated).toEqual([prepared]);
    expect(prepared.loads).toEqual([audioPath("first-audio")]);
    expect(prepared.played).toEqual([audioPath("first-audio")]);
    expect(player.snapshot().current?.id).toBe("first");
    expect(player.snapshot().playing).toBe(true);
    expect(player.queueSnapshot().currentEntry).toBe(entries[0].id);
    expect((await player.queuePage(0, 10)).entries).toEqual(entries);
    expect(fixture.requests.every(({ purpose }) => purpose === "cached")).toBe(
      true,
    );
    const following = await fixture.buffer("last-audio");
    expect(following.paused).toBe(true);
    expect(fixture.owned.size).toBe(2);
  } finally {
    await fixture.close();
  }
});

test("duplicate occurrences keep their IDs while adopting the same audio without repeating provider resolution", async () => {
  let available = false;
  const fixture = browserPreparationModes(async (id, options) => {
    if (id !== "same") throw new Error("Unexpected queue recording");
    if (options.purpose === "resolve") available = true;
    return available ? [{ id: "same-audio", ready: true }] : [];
  });
  const { player, initial } = fixture;

  try {
    await player.playQueue([recording("same"), recording("same")]);
    const entries = (await player.queuePage(0, 10)).entries;
    expect(entries[0].id).not.toBe(entries[1].id);
    expect(player.queueSnapshot().currentEntry).toBe(entries[0].id);
    const prepared = await fixture.buffer("same-audio");
    expect(prepared.src).toBe(audioPath("same-audio"));
    expect(prepared.paused).toBe(true);
    expect(prepared.played).toEqual([]);
    prepared.ready();
    initial.complete();
    await prepared.playing;

    expect(fixture.activated).toEqual([prepared]);
    expect(prepared.loads).toEqual([audioPath("same-audio")]);
    expect(prepared.played).toEqual([audioPath("same-audio")]);
    expect(initial.played).toEqual([audioPath("same-audio")]);
    expect(player.snapshot().current?.id).toBe("same");
    expect(player.snapshot().playing).toBe(true);
    expect(player.queueSnapshot().currentEntry).toBe(entries[1].id);
    expect((await player.queuePage(0, 10)).entries).toEqual(entries);
    expect(fixture.requests.map(({ purpose }) => purpose)).toEqual([
      "cached",
      "refresh",
      "resolve",
      "cached",
    ]);
    expect(fixture.owned.size).toBe(1);
  } finally {
    await fixture.close();
  }
});
