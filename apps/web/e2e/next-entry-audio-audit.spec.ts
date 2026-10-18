import { expect, test } from "@playwright/test";
import {
  auditAudioRequests,
  type AudioRequestEvidence,
} from "./fixtures/next-entry-audio";

test.use({ trace: "off", screenshot: "off", video: "off" });

const first =
  "http://127.0.0.1:8080/api/v1/sources/11111111-1111-1111-1111-111111111111/audio";
const next =
  "http://127.0.0.1:8080/api/v1/sources/22222222-2222-2222-2222-222222222222/audio";
const expected = [first, next];

function probe(): AudioRequestEvidence {
  return {
    url: first,
    method: "GET",
    resourceType: "other",
    started: 1,
    routed: 0,
    continued: 0,
    responded: 2,
    finished: 3,
    failed: false,
    responses: 1,
    status: 0,
    headersEmpty: true,
    audioContentType: false,
    bodyBytes: 0,
    bodyVerified: false,
  };
}

function actual(): AudioRequestEvidence {
  return {
    ...probe(),
    started: 4,
    routed: 5,
    continued: 6,
    responded: 7,
    finished: 8,
    status: 200,
    headersEmpty: false,
    audioContentType: true,
    bodyBytes: 640044,
    bodyVerified: true,
  };
}

test("a completed WebKit media delegate probe requires later routed exact audio for the same source", () => {
  expect(auditAudioRequests([probe(), actual()], expected, "webkit")).toEqual({
    violations: 0,
    pairedProbes: 1,
    unpairedProbes: 0,
  });
});

test("known inspector absence can pair a completed WebKit delegate with later verified audio while its body size stays unknown", () => {
  const absent: AudioRequestEvidence = {
    ...probe(),
    bodyBytes: null,
    bodyReadFailure: "no_resource_data",
  };
  expect(auditAudioRequests([absent, actual()], expected, "webkit")).toEqual({
    violations: 0,
    pairedProbes: 1,
    unpairedProbes: 0,
  });
  expect(absent.bodyBytes).toBeNull();
});

const rejectedProbes: [string, Partial<AudioRequestEvidence>][] = [
  ["a routed status-zero response fails", { routed: 2, continued: 3 }],
  ["a status-zero response with headers fails", { headersEmpty: false }],
  [
    "a status-zero response with unavailable body evidence fails",
    { bodyBytes: null },
  ],
  ["a status-zero response with body bytes fails", { bodyBytes: 1 }],
  [
    "a missing resource cannot supply delegate body evidence",
    { bodyBytes: null, bodyReadFailure: "no_resource" },
  ],
  [
    "evicted content cannot supply delegate body evidence",
    { bodyBytes: null, bodyReadFailure: "evicted" },
  ],
  [
    "an unknown body failure cannot supply delegate body evidence",
    { bodyBytes: null, bodyReadFailure: "other" },
  ],
  [
    "missing content cannot claim observed empty bytes",
    { bodyBytes: 0, bodyReadFailure: "no_resource_data" },
  ],
  [
    "missing content cannot accompany observed body bytes",
    { bodyBytes: 1, bodyReadFailure: "no_resource_data" },
  ],
  ["a failed status-zero request fails", { failed: true }],
  ["an unfinished status-zero request fails", { finished: 0 }],
  [
    "another request method cannot supply a media delegate probe",
    { method: "POST" },
  ],
  [
    "another resource type cannot supply a media delegate probe",
    { resourceType: "fetch" },
  ],
];
for (const [description, changed] of rejectedProbes) {
  test(description, () => {
    expect(
      auditAudioRequests(
        [{ ...probe(), ...changed }, actual()],
        expected,
        "webkit",
      ).violations,
    ).toBe(1);
  });
}

test("a probe without a later verified audio response fails", () => {
  expect(auditAudioRequests([probe()], expected, "webkit")).toEqual({
    violations: 1,
    pairedProbes: 0,
    unpairedProbes: 1,
  });
  expect(
    auditAudioRequests(
      [probe(), { ...actual(), bodyVerified: false }],
      expected,
      "webkit",
    ).violations,
  ).toBeGreaterThan(0);
});

test("real audio for another expected source cannot pair a probe", () => {
  expect(
    auditAudioRequests(
      [probe(), { ...actual(), url: next }],
      expected,
      "webkit",
    ).violations,
  ).toBe(1);
});

test("one real audio response cannot authorize two delegate probes", () => {
  const second = { ...probe(), started: 4, responded: 5, finished: 6 };
  const later = {
    ...actual(),
    started: 7,
    routed: 8,
    continued: 9,
    responded: 10,
    finished: 11,
  };
  expect(
    auditAudioRequests([probe(), second, later], expected, "webkit"),
  ).toEqual({
    violations: 1,
    pairedProbes: 1,
    unpairedProbes: 1,
  });
});

test("another browser cannot classify a status-zero response as a delegate probe", () => {
  expect(
    auditAudioRequests([probe(), actual()], expected, "chromium").violations,
  ).toBe(1);
});

test("a foreign audio URL cannot supply either side of a verified pair", () => {
  const foreign = first.replace("127.0.0.1", "provider.invalid");
  expect(
    auditAudioRequests(
      [
        { ...probe(), url: foreign },
        { ...actual(), url: foreign },
      ],
      expected,
      "webkit",
    ).violations,
  ).toBe(2);
});

test("an earlier real audio request cannot authorize a later delegate probe", () => {
  const earlier = {
    ...actual(),
    started: 2,
    routed: 3,
    continued: 4,
    responded: 5,
    finished: 6,
  };
  expect(
    auditAudioRequests([probe(), earlier], expected, "webkit").violations,
  ).toBe(1);
});

test("unexpected HTTP audio statuses fail", () => {
  expect(
    auditAudioRequests([{ ...actual(), status: 404 }], expected, "webkit")
      .violations,
  ).toBe(1);
});

test("a body-read failure on actual audio cannot authorize a delegate probe", () => {
  expect(
    auditAudioRequests(
      [probe(), { ...actual(), bodyReadFailure: "no_resource_data" }],
      expected,
      "webkit",
    ).violations,
  ).toBe(2);
});
