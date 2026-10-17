import { expect, test } from "@playwright/test";
import {
  classifyAudioCancellations,
  type AudioCancellationObservation,
} from "./fixtures/youtube-audio-cancellations";

function tail(request: number, start: number): AudioCancellationObservation {
  return {
    request,
    source_group: 1,
    method: 1,
    canonical_url: 1,
    is_navigation: 0,
    request_service_worker: 0,
    response_service_worker: 0,
    response_matches_request: 1,
    status: 206,
    audio_framing_valid: 1,
    content_length: 4096 - start,
    request_range_kind: 2,
    request_range_start: start,
    request_range_end: -1,
    response_range_start: start,
    response_range_end: 4095,
    response_range_total: 4096,
    requested_at_ms: request * 10,
    responded_at_ms: request * 10 + 1,
    terminated_at_ms: request * 10 + 9,
    finished: 1,
    failed: 0,
    failure_exact_code: 0,
    terminal_during_cleanup: 0,
  };
}

const observations: AudioCancellationObservation[] = [
  {
    ...tail(1, 0),
    status: 200,
    request_range_kind: 0,
    request_range_start: -1,
    finished: 0,
    failed: 1,
    failure_exact_code: 5,
  },
  tail(2, 4080),
  { ...tail(3, 1024), finished: 0, failed: 1, failure_exact_code: 5 },
  tail(4, 2048),
];

function changed(index: number, patch: Partial<AudioCancellationObservation>) {
  return observations.map((value, position) =>
    position === index ? { ...value, ...patch } : { ...value },
  );
}

test("only the observed WebKit replacement chain separates browser cancellations from unexpected failures", () => {
  const cases: {
    name: string;
    browser: string;
    values: AudioCancellationObservation[];
    raw: number;
    accepted?: number;
  }[] = [
    {
      name: "normal completion",
      browser: "webkit",
      values: [tail(1, 0)],
      raw: 0,
      accepted: 0,
    },
    {
      name: "initial request, EOF probe, progressive replacement and normal final tail",
      browser: "webkit",
      values: observations,
      raw: 2,
      accepted: 2,
    },
    {
      name: "Chromium cancellation",
      browser: "chromium",
      values: observations,
      raw: 2,
    },
    {
      name: "Firefox cancellation",
      browser: "firefox",
      values: observations,
      raw: 2,
    },
    {
      name: "nonexact cancellation code",
      browser: "webkit",
      values: changed(2, { failure_exact_code: 4 }),
      raw: 2,
    },
    {
      name: "deadline or unknown transport failure",
      browser: "webkit",
      values: changed(2, { failure_exact_code: 0 }),
      raw: 2,
    },
    {
      name: "closed request range",
      browser: "webkit",
      values: changed(2, { request_range_kind: 1, request_range_end: 4095 }),
      raw: 2,
    },
    {
      name: "backward restart",
      browser: "webkit",
      values: changed(3, {
        request_range_start: 512,
        response_range_start: 512,
        content_length: 3584,
      }),
      raw: 2,
    },
    {
      name: "request and response ranges disagree",
      browser: "webkit",
      values: changed(3, { request_range_start: 2047 }),
      raw: 2,
    },
    {
      name: "malformed audio framing",
      browser: "webkit",
      values: changed(3, { audio_framing_valid: 0 }),
      raw: 2,
    },
    {
      name: "HTTP failure after cancellation",
      browser: "webkit",
      values: changed(3, { status: 500 }),
      raw: 2,
    },
    {
      name: "different private source URL",
      browser: "webkit",
      values: changed(3, { source_group: 2 }),
      raw: 2,
    },
    {
      name: "changed representation length",
      browser: "webkit",
      values: changed(3, {
        response_range_total: 8192,
        response_range_end: 8191,
        content_length: 6144,
      }),
      raw: 2,
    },
    {
      name: "probe without actual data resumption",
      browser: "webkit",
      values: observations.slice(0, 2),
      raw: 1,
    },
    {
      name: "no final normal tail",
      browser: "webkit",
      values: observations.slice(0, 3),
      raw: 2,
    },
    {
      name: "final tail still pending",
      browser: "webkit",
      values: changed(3, { finished: 0, terminated_at_ms: -1 }),
      raw: 2,
    },
    {
      name: "cancellation during cleanup",
      browser: "webkit",
      values: changed(2, { terminal_during_cleanup: 1 }),
      raw: 2,
    },
  ];
  for (const value of cases) {
    const result = classifyAudioCancellations(value.browser, value.values);
    expect(result.audio_request_failures, value.name).toBe(value.raw);
    expect(
      result.audio_browser_cancellations +
        result.audio_unexpected_request_failures,
      value.name,
    ).toBe(value.raw);
    if (value.accepted !== undefined) {
      expect(result.audio_browser_cancellations, value.name).toBe(
        value.accepted,
      );
      expect(result.audio_unexpected_request_failures, value.name).toBe(
        value.raw - value.accepted,
      );
    } else {
      expect(
        result.audio_unexpected_request_failures,
        value.name,
      ).toBeGreaterThan(0);
    }
  }
});
