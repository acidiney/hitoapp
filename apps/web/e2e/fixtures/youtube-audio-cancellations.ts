export type AudioCancellationObservation = {
  request: number;
  source_group: number;
  method: number;
  canonical_url: number;
  is_navigation: number;
  request_service_worker: number;
  response_service_worker: number;
  response_matches_request: number;
  status: number;
  audio_framing_valid: number;
  content_length: number;
  request_range_kind: number;
  request_range_start: number;
  request_range_end: number;
  response_range_start: number;
  response_range_end: number;
  response_range_total: number;
  requested_at_ms: number;
  responded_at_ms: number;
  terminated_at_ms: number;
  finished: number;
  failed: number;
  failure_exact_code: number;
  terminal_during_cleanup: number;
};

export function classifyAudioCancellations(
  browser: string,
  observations: readonly AudioCancellationObservation[],
) {
  const failures = observations.filter((value) => value.failed === 1);
  const ordered = [...observations].sort(
    (left, right) => left.request - right.request,
  );
  const validOrder =
    observations.length <= 32 &&
    new Set(ordered.map((value) => value.request)).size === ordered.length &&
    ordered.every(
      (value) => Number.isSafeInteger(value.request) && value.request > 0,
    );
  const accepted =
    browser === "webkit" && validOrder
      ? failures.filter((value) => completedReplacementChain(value, ordered))
          .length
      : 0;
  return {
    audio_request_failures: failures.length,
    audio_browser_cancellations: accepted,
    audio_unexpected_request_failures: failures.length - accepted,
  };
}

function validResponse(value: AudioCancellationObservation) {
  const start = value.response_range_start;
  const total = value.response_range_total;
  return (
    value.method === 1 &&
    value.canonical_url === 1 &&
    value.is_navigation === 0 &&
    value.request_service_worker === 0 &&
    value.response_service_worker === 0 &&
    value.response_matches_request === 1 &&
    value.audio_framing_valid === 1 &&
    Number.isSafeInteger(value.source_group) &&
    value.source_group > 0 &&
    Number.isSafeInteger(start) &&
    start >= 0 &&
    Number.isSafeInteger(total) &&
    total > start &&
    total <= 128 * 1024 * 1024 &&
    value.response_range_end === total - 1 &&
    value.content_length === total - start &&
    Number.isSafeInteger(value.requested_at_ms) &&
    value.requested_at_ms >= 0 &&
    Number.isSafeInteger(value.responded_at_ms) &&
    value.responded_at_ms >= value.requested_at_ms &&
    ((value.status === 200 &&
      start === 0 &&
      value.request_range_kind === 0 &&
      value.request_range_start === -1 &&
      value.request_range_end === -1) ||
      (value.status === 206 &&
        value.request_range_kind === 2 &&
        value.request_range_start === start &&
        value.request_range_end === -1))
  );
}

function terminated(value: AudioCancellationObservation) {
  return (
    value.terminal_during_cleanup === 0 &&
    Number.isSafeInteger(value.terminated_at_ms) &&
    value.terminated_at_ms >= value.responded_at_ms
  );
}

function completedProbe(value: AudioCancellationObservation) {
  return (
    validResponse(value) &&
    terminated(value) &&
    value.status === 206 &&
    value.content_length === 16 &&
    value.failed === 0 &&
    value.finished === 1 &&
    value.failure_exact_code === 0
  );
}

function completedReplacementChain(
  initial: AudioCancellationObservation,
  ordered: readonly AudioCancellationObservation[],
) {
  let current = initial;
  for (let step = 0; step < ordered.length; step++) {
    if (!validResponse(current) || !terminated(current)) return false;
    if (current.failed === 0)
      return (
        current.status === 206 &&
        current.finished === 1 &&
        current.failure_exact_code === 0 &&
        current.content_length > 16
      );
    if (
      current.failed !== 1 ||
      current.finished !== 0 ||
      current.failure_exact_code !== 5
    )
      return false;
    let next = ordered.find((value) => value.request > current.request);
    let boundary = current;
    if (current.status === 200 && next && completedProbe(next)) {
      if (
        next.source_group !== current.source_group ||
        next.response_range_total !== current.response_range_total ||
        next.requested_at_ms < current.terminated_at_ms
      )
        return false;
      boundary = next;
      next = ordered.find((value) => value.request > boundary.request);
      if (!next || next.response_range_start >= boundary.response_range_start)
        return false;
    }
    if (
      !next ||
      !validResponse(next) ||
      next.status !== 206 ||
      next.source_group !== current.source_group ||
      next.response_range_total !== current.response_range_total ||
      next.response_range_start <= current.response_range_start ||
      next.requested_at_ms < boundary.terminated_at_ms
    )
      return false;
    current = next;
  }
  return false;
}
