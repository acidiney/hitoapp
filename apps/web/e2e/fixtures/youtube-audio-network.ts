import type { Page, Request } from "@playwright/test";

function validAudioFraming(status: number, headers: Record<string, string>) {
  const length = headers["content-length"] ?? "";
  if (
    !/^[0-9]+$/.test(length) ||
    !Number.isSafeInteger(Number(length)) ||
    Number(length) <= 0 ||
    Number(length) > 128 * 1024 * 1024 ||
    !/^audio\/mp4(?:\s*;|$)/i.test(headers["content-type"] ?? "") ||
    "transfer-encoding" in headers
  )
    return false;
  if (status === 200) return !("content-range" in headers);
  if (status !== 206) return false;
  const range = /^bytes ([0-9]+)-([0-9]+)\/([0-9]+)$/.exec(
    headers["content-range"] ?? "",
  );
  if (!range) return false;
  const [start, end, total] = range.slice(1).map(Number);
  return (
    [start, end, total].every(Number.isSafeInteger) &&
    start >= 0 &&
    start <= end &&
    end < total &&
    total <= 128 * 1024 * 1024 &&
    end - start + 1 === Number(length)
  );
}

export function liveAudioRequestDiagnostics(
  page: Page,
  origin: string | null,
  phase: () => number,
) {
  type Entry = {
    request: number;
    phase: number;
    method: number;
    is_navigation: number;
    request_service_worker: number;
    resource_type: number;
    resource_other: number;
    canonical_url: number;
    positive_same_url: number;
    status: number;
    response_phase: number;
    response_service_worker: number;
    response_matches_request: number;
    response_header_count: number;
    content_length_present: number;
    content_length_valid: number;
    content_length: number;
    content_range_present: number;
    transfer_encoding_present: number;
    content_type_present: number;
    content_type_audio_mp4: number;
    audio_framing_valid: number;
    finished: number;
    failed: number;
    terminal_phase: number;
    failure_category: number;
    failure_length: number;
    terminal_during_cleanup: number;
  };
  const requests = new Map<Request, Entry>();
  let cleanup = false;
  let overflow = 0;
  const validAudioResponse = (observation: Entry) =>
    [200, 206].includes(observation.status) &&
    observation.method === 1 &&
    observation.canonical_url === 1 &&
    observation.response_matches_request === 1 &&
    observation.is_navigation === 0 &&
    observation.request_service_worker === 0 &&
    observation.response_service_worker === 0 &&
    observation.audio_framing_valid === 1;
  const syntheticWebKitResponse = (
    browser: string,
    request: Request,
    observation: Entry,
  ) =>
    browser === "webkit" &&
    observation.status === 0 &&
    observation.method === 1 &&
    observation.canonical_url === 1 &&
    observation.resource_type === 0 &&
    observation.resource_other === 1 &&
    observation.is_navigation === 0 &&
    observation.request_service_worker === 0 &&
    observation.response_service_worker === 0 &&
    observation.response_matches_request === 1 &&
    observation.response_header_count === 0 &&
    observation.content_length_present === 0 &&
    observation.content_range_present === 0 &&
    observation.transfer_encoding_present === 0 &&
    observation.content_type_present === 0 &&
    observation.finished === 1 &&
    observation.failed === 0 &&
    observation.terminal_during_cleanup === 0 &&
    Array.from(requests).some(
      ([positiveRequest, positive]) =>
        positiveRequest.url() === request.url() && validAudioResponse(positive),
    );
  const entry = (request: Request) => {
    const existing = requests.get(request);
    if (existing) return existing;
    const url = new URL(request.url());
    if (
      url.origin !== origin ||
      !/^\/api\/v1\/sources\/[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}\/audio$/.test(
        url.pathname,
      )
    )
      return undefined;
    if (requests.size >= 32) {
      overflow++;
      return undefined;
    }
    const type = request.resourceType();
    const observation: Entry = {
      request: requests.size + 1,
      phase: phase(),
      method:
        request.method() === "GET" ? 1 : request.method() === "HEAD" ? 2 : 0,
      is_navigation: Number(request.isNavigationRequest()),
      request_service_worker: Number(request.serviceWorker() !== null),
      resource_type:
        type === "media" ? 1 : type === "fetch" ? 2 : type === "xhr" ? 3 : 0,
      resource_other: Number(type === "other"),
      canonical_url: Number(!url.search && !url.hash),
      positive_same_url: Number(
        Array.from(requests).some(
          ([prior, observation]) =>
            prior.url() === request.url() &&
            [200, 206].includes(observation.status),
        ),
      ),
      status: -1,
      response_phase: 0,
      response_service_worker: -1,
      response_matches_request: -1,
      response_header_count: -1,
      content_length_present: -1,
      content_length_valid: -1,
      content_length: -1,
      content_range_present: -1,
      transfer_encoding_present: -1,
      content_type_present: -1,
      content_type_audio_mp4: -1,
      audio_framing_valid: -1,
      finished: 0,
      failed: 0,
      terminal_phase: 0,
      failure_category: 0,
      failure_length: 0,
      terminal_during_cleanup: 0,
    };
    requests.set(request, observation);
    return observation;
  };
  const report = (kind: number, observation: Entry) =>
    console.info(
      JSON.stringify({
        event: "youtube_live_audio_request_diagnostic",
        kind,
        ...observation,
      }),
    );
  page.on("request", (request) => {
    entry(request);
  });
  page.on("response", (response) => {
    const observation = entry(response.request());
    if (!observation) return;
    observation.status = response.status();
    observation.response_phase = phase();
    observation.response_service_worker = Number(response.fromServiceWorker());
    observation.response_matches_request = Number(
      response.url() === response.request().url(),
    );
    const headers = response.headers();
    const length = headers["content-length"];
    const validLength =
      length !== undefined &&
      /^[0-9]+$/.test(length) &&
      Number.isSafeInteger(Number(length));
    observation.response_header_count = Object.keys(headers).length;
    observation.content_length_present = Number(length !== undefined);
    observation.content_length_valid = Number(validLength);
    observation.content_length = validLength ? Number(length) : -1;
    observation.content_range_present = Number("content-range" in headers);
    observation.transfer_encoding_present = Number(
      "transfer-encoding" in headers,
    );
    observation.content_type_present = Number("content-type" in headers);
    observation.content_type_audio_mp4 = Number(
      /^audio\/mp4(?:\s*;|$)/i.test(headers["content-type"] ?? ""),
    );
    observation.audio_framing_valid = Number(
      validAudioFraming(observation.status, headers),
    );
    if ([200, 206].includes(observation.status)) {
      for (const [request, matching] of requests) {
        if (request.url() === response.request().url())
          matching.positive_same_url = 1;
      }
    }
    report(1, observation);
  });
  page.on("requestfinished", (request) => {
    const observation = requests.get(request);
    if (!observation) return;
    observation.finished = 1;
    observation.terminal_phase = phase();
    observation.terminal_during_cleanup = Number(cleanup);
    report(2, observation);
  });
  page.on("requestfailed", (request) => {
    const observation = requests.get(request);
    if (!observation) return;
    const failure = request.failure()?.errorText.toLowerCase().trim() ?? "";
    observation.failed = 1;
    observation.terminal_phase = phase();
    observation.terminal_during_cleanup = Number(cleanup);
    observation.failure_length = failure.length;
    observation.failure_category =
      failure === "net::err_aborted"
        ? 1
        : failure === "ns_binding_aborted"
          ? 2
          : failure === "cancelled" || failure === "canceled"
            ? 3
            : failure.includes("cancel")
              ? 4
              : failure.includes("interrupt")
                ? 5
                : 9;
    report(3, observation);
  });
  return {
    summary: (browser: string) => {
      const entries = Array.from(requests);
      const positive = entries.filter(([, observation]) =>
        [200, 206].includes(observation.status),
      );
      const synthetic = entries.filter(([request, observation]) =>
        syntheticWebKitResponse(browser, request, observation),
      );
      return {
        audio_responses: positive.length,
        audio_zero_responses: entries.filter(([, value]) => value.status === 0)
          .length,
        audio_synthetic_responses: synthetic.length,
        audio_http_failures: entries.filter(
          ([request, observation]) =>
            observation.status >= 0 &&
            ![200, 206].includes(observation.status) &&
            !syntheticWebKitResponse(browser, request, observation),
        ).length,
        audio_framing_failures: positive.filter(
          ([, observation]) => !validAudioResponse(observation),
        ).length,
        audio_request_failures: entries.filter(
          ([, value]) => value.failed === 1,
        ).length,
        audio_observation_overflow: overflow,
        audio_statuses: entries
          .map(([, observation]) => observation.status)
          .filter((status) => status >= 0)
          .slice(-8),
      };
    },
    snapshot: () =>
      Array.from(requests.values(), (observation) => ({ ...observation })),
    markCleanup: () => {
      cleanup = true;
    },
  };
}
