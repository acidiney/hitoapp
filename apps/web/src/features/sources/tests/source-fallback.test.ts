import { expect, test } from "vitest";
import { PlaybackSources } from "../../playback/api/playbackSources";
import { MAX_FALLBACK_ATTEMPTS } from "../model/sourceFallback";

const ready = (count: number) =>
  Array.from({ length: count }, (_, index) => ({
    id: `source-${index}`,
    ready: true,
  }));

async function visit(sources: PlaybackSources) {
  const visited: string[] = [];
  for (let step = 0; step < 50; step++) {
    const next = await sources.next();
    if (!next) break;
    visited.push(next.id);
  }
  return visited;
}

test("fallback visits each ready source once and stops at the attempt limit", async () => {
  const many = ready(MAX_FALLBACK_ATTEMPTS + 4);
  const sources = new PlaybackSources("recording", async () => many);
  expect(await visit(sources)).toEqual(
    many.slice(0, MAX_FALLBACK_ATTEMPTS).map((source) => source.id),
  );
  expect(await sources.next()).toBeUndefined();
});

test("with fallback off the first source is offered and a failure does not move to the next", async () => {
  const resolve = async () => ready(3);
  const allowed = async () => false;
  const sources = new PlaybackSources("recording", resolve, undefined, allowed);
  expect(await visit(sources)).toEqual(["source-0"]);
});

test("with fallback on a failure moves to the next source", async () => {
  const resolve = async (_recording: string, options: { purpose: string }) =>
    options.purpose === "cached" ? ready(3) : [];
  const allowed = async () => true;
  const sources = new PlaybackSources("recording", resolve, undefined, allowed);
  expect(await visit(sources)).toEqual(["source-0", "source-1", "source-2"]);
});
