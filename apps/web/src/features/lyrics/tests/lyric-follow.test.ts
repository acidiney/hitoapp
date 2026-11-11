import { expect, test } from "vitest";
import { followableLines, selectedLyricsOutcome } from "../model/lyricContent";
import {
  readyV1,
  readyV2,
  reviewedIdentity,
} from "../fixtures/lyricIdentityWire";

const identity = reviewedIdentity("aaaa0000-0000-4000-8000-000000000001", 1);

const anchored = (duration: number | null | undefined) =>
  selectedLyricsOutcome(
    readyV2(identity, {
      id: "d",
      rendition_id: "r",
      duration_ms: duration,
      content: {
        kind: "anchored",
        lines: [
          { start_ms: 0, text: "" },
          { start_ms: 1000, text: "One" },
          { start_ms: 4000, text: "" },
          { start_ms: 5000, text: "Two" },
          { start_ms: 9000, text: "" },
        ],
      },
    }),
  );

function viewOf(outcome: ReturnType<typeof anchored>) {
  if (outcome.kind !== "lyrics") throw new Error("expected lyrics");
  return outcome.view;
}

test("anchored lyrics follow the music only when the document duration is within 2 seconds of the audio", () => {
  const near = followableLines(viewOf(anchored(226_000)), 225_800);
  expect(near?.map((line) => [line.start_ms, line.text])).toEqual([
    [1000, "One"],
    [5000, "Two"],
  ]);
  expect(followableLines(viewOf(anchored(226_000)), 223_000)).toBeNull();
  expect(followableLines(viewOf(anchored(227_900)), 226_000)).not.toBeNull();
  expect(followableLines(viewOf(anchored(null)), 226_000)).toBeNull();
  expect(followableLines(viewOf(anchored(undefined)), 226_000)).toBeNull();
  expect(followableLines(viewOf(anchored(226_000)), 0)).toBeNull();
});

test("blank clear events end the previous line and are never lines", () => {
  const lines = followableLines(viewOf(anchored(10_000)), 10_000)!;
  expect(lines.map((line) => [line.text, line.start_ms, line.end_ms])).toEqual([
    ["One", 1000, 4000],
    ["Two", 5000, 9000],
  ]);
});

test("timed v1 lines keep following, but a present document duration that disagrees with the audio keeps them static", () => {
  const timed = (duration?: number) =>
    viewOf(
      selectedLyricsOutcome(
        readyV1(identity, {
          id: "l",
          rendition_id: "r",
          duration_ms: duration,
          lines: [
            { start_ms: 0, end_ms: 1000, text: "A" },
            { start_ms: 1000, end_ms: 2000, text: "B" },
          ],
          attribution: null,
        }),
      ),
    );
  expect(followableLines(timed(), 200_000)).toHaveLength(2);
  expect(followableLines(timed(200_000), 201_500)).toHaveLength(2);
  expect(followableLines(timed(200_000), 230_000)).toBeNull();
});

test("anchors that do not move forward are never treated as timing", () => {
  const flat = viewOf(
    selectedLyricsOutcome(
      readyV2(identity, {
        id: "d",
        rendition_id: "r",
        duration_ms: 10_000,
        content: {
          kind: "anchored",
          lines: [
            { start_ms: 0, text: "A" },
            { start_ms: 0, text: "B" },
          ],
        },
      }),
    ),
  );
  expect(followableLines(flat, 10_000)).toBeNull();
});
