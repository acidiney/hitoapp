import { expect, test } from "vitest";
import {
  previewLines,
  readMatchesIdentity,
  selectedLyricsOutcome,
  type ReadyLookup,
} from "../model/lyricContent";
import {
  alreadySaved,
  identityNames,
  isRemovable,
  selectorKey,
} from "../model/lyricIdentity";
import { HttpError } from "../../../shared/http";
import { lyricFailure } from "../model/lyricFailure";
import { lyricQuerySeed } from "../hooks/useLyricQuery";
import {
  observationIdentity,
  plainDocument,
  readyV1,
  readyV2,
  recordingId,
  reviewedIdentity,
} from "../fixtures/lyricIdentityWire";

const identity = reviewedIdentity("22220000-0000-4000-8000-000000000001", 2);

test("a ready v2 document with null lyrics is not_found while the other statuses map one to one", () => {
  expect(selectedLyricsOutcome(readyV2(identity, null))).toEqual({
    kind: "not_found",
  });
  expect(selectedLyricsOutcome(readyV1(identity, null))).toEqual({
    kind: "not_found",
  });
  for (const status of ["unsupported", "unavailable", "cancelled"] as const)
    expect(selectedLyricsOutcome({ status })).toEqual({ kind: status });
});

test("plain content keeps every line including blanks and ignores creators", () => {
  const outcome = selectedLyricsOutcome(
    readyV2(
      identity,
      plainDocument(["A", "", "", "B"], {
        provider_label: "Provider",
        content_url: "https://example.test/x",
        creators: ["Someone"],
      }),
    ),
  );
  if (outcome.kind !== "lyrics") throw new Error("expected lyrics");
  expect(outcome.view.lines.map((line) => line.text)).toEqual([
    "A",
    "",
    "",
    "B",
  ]);
  expect(outcome.view.attribution).toEqual({
    providerLabel: "Provider",
    contentUrl: "https://example.test/x",
  });
  expect(outcome.view.synced).toBe(false);
  expect(previewLines(outcome.view)).toEqual(["A", "", "", "B"]);
});

test("a ready reply must echo the request recording, exact selector and provider reference", () => {
  const ready = readyV2(identity, plainDocument(["x"])) as ReadyLookup;
  expect(readMatchesIdentity(ready, recordingId, identity)).toBe(true);
  expect(readMatchesIdentity(ready, "another", identity)).toBe(false);
  const otherSelector = reviewedIdentity(
    "22220000-0000-4000-8000-000000000001",
    3,
  );
  expect(readMatchesIdentity(ready, recordingId, otherSelector)).toBe(false);
  const otherRef = {
    ...identity,
    provider_recording: { id: "different", revision: null },
  };
  expect(readMatchesIdentity(ready, recordingId, otherRef)).toBe(false);
  const wrongDocument = {
    ...ready,
    document: { ...ready.document, recording: { id: "different" } },
  } as ReadyLookup;
  expect(readMatchesIdentity(wrongDocument, recordingId, identity)).toBe(false);
});

test("selectors never turn an identity UUID into an observation UUID and only reviewed identities are removable", () => {
  expect(selectorKey(identity.selector)).toBe(
    "reviewed:22220000-0000-4000-8000-000000000001:2",
  );
  expect(selectorKey(observationIdentity.selector)).toMatch(/^observation:/);
  expect(isRemovable(identity)).toBe(true);
  expect(isRemovable(observationIdentity)).toBe(false);
});

test("repeated service labels get ordinals so choices stay distinguishable", () => {
  const second = reviewedIdentity("22220000-0000-4000-8000-000000000002", 1);
  const names = identityNames([identity, second, observationIdentity]);
  expect(Object.values(names)).toEqual([
    "Open lyrics (1)",
    "Open lyrics (2)",
    "Second lyrics",
  ]);
  expect(
    alreadySaved(
      [identity],
      identity.installation_id,
      identity.provider_recording.id,
    ),
  ).toBe(true);
  expect(
    alreadySaved([identity], "other", identity.provider_recording.id),
  ).toBe(false);
});

test("failures map to the table without leaking server text for non-conflict errors", () => {
  const status = (value: number, message = "server words") =>
    lyricFailure(new HttpError(value, message), "read");
  for (const value of [401, 403, 423])
    expect(status(value)).toMatchObject({ kind: "access", reload: false });
  expect(status(404)).toMatchObject({ kind: "gone", reload: true });
  expect(status(409, "Lyric limit reached")).toEqual({
    kind: "conflict",
    message: "Lyric limit reached",
    reload: true,
  });
  expect(status(502).message).not.toContain("server words");
  expect(lyricFailure(new Error("x"), "search").kind).toBe("temporary");
});

test("the query seed joins committed title and artist and skips blanks", () => {
  expect(lyricQuerySeed(" Song ", " Artist ")).toBe("Song Artist");
  expect(lyricQuerySeed("Song", "  ")).toBe("Song");
});
