import { isDeepStrictEqual } from "node:util";
import { expect, type Page } from "@playwright/test";
import type { components } from "../../src/generated";
import type { LibraryFixture } from "./audiobook-library-state";
import type { LibraryDelivery } from "./audiobook-library-delivery";
import type { LibraryEvidence } from "./audiobook-library-evidence";

type Schema = components["schemas"];
type Snapshot = Awaited<ReturnType<LibraryFixture["snapshot"]>>;
export function unchanged(before: unknown, after: unknown) {
  expect(isDeepStrictEqual(before, after)).toBe(true);
}

export function explicitChanges(
  before: Snapshot,
  after: Snapshot,
  operations: Schema["Operation"][],
  books: Schema["Playlist"][],
  playlists: Schema["Playlist"][],
) {
  const expected = structuredClone(before);
  const extra = after.owner.events.slice(before.owner.events.length);
  expect(extra.length).toBe(operations.length);
  expect(
    extra.every(
      (event, index) =>
        "operation" in event &&
        event.device_id === "owned-library-browser-fixture" &&
        isDeepStrictEqual(event.operation, operations[index]) &&
        event.sequence > (before.owner.events.at(-1)?.sequence ?? 0),
    ),
  ).toBe(true);
  expected.owner.events.push(...extra);
  expected.owner.books = books;
  expected.owner.playlists = playlists;
  unchanged(expected, after);
}

export async function noListening(
  page: Page,
  delivery: LibraryDelivery,
  evidence: LibraryEvidence,
  recovery: boolean,
) {
  await page.waitForLoadState("networkidle");
  await delivery.finish();
  await expect
    .poll(
      () =>
        evidence.counts.started > 0 &&
        evidence.counts.started === evidence.counts.finished,
    )
    .toBe(true);
  const counts = delivery.counts;
  for (const key of [
    "apiBookReads",
    "sync",
    "audio",
    "source",
    "downloads",
    "foreign",
    "unexpectedWrites",
    "unexpectedReads",
    "failures",
    "preparationFailures",
    "invalid",
  ] as const)
    expect(counts[key]).toBe(0);
  expect(counts.absentDownloadStatuses).toBe(counts.downloadStatusReads);
  expect(counts.ownedFailures).toBe(recovery ? 3 : 0);
  expect(counts.lost).toBe(recovery ? 1 : 0);
  expect(counts.held).toBe(recovery ? 2 : 0);
  expect(counts.forwarded).toBe(recovery ? 3 : 0);
  expect(counts.acceptedSessionResponses).toBe(recovery ? 2 : 0);
  expect(counts.sessionCancellations).toBeLessThanOrEqual(recovery ? 2 : 0);
  expect(counts.sessionVerificationCancellations).toBeLessThanOrEqual(
    recovery ? 1 : 0,
  );
  expect(counts.preparationCancellations).toBeLessThanOrEqual(2);
  expect(counts.visits).toBeGreaterThanOrEqual(recovery ? 12 : 8);
  await expect(page.locator("audio[data-hito-player]")).toHaveCount(1);
  expect(
    await page.locator("audio[data-hito-player]").evaluateAll((nodes) =>
      nodes.every((node) => {
        const audio = node as HTMLAudioElement;
        return (
          audio.paused && audio.currentTime === 0 && audio.currentSrc === ""
        );
      }),
    ),
  ).toBe(true);
}
