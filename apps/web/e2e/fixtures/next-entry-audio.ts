import type { BrowserContext, Request, Response } from "@playwright/test";

export type AudioRequestEvidence = {
  url: string;
  method: string;
  resourceType: string;
  started: number;
  routed: number;
  continued: number;
  responded: number;
  finished: number;
  failed: boolean;
  responses: number;
  status: number;
  headersEmpty: boolean;
  audioContentType: boolean;
  bodyBytes: number | null;
  bodyVerified: boolean;
  bodyReadFailure?: InspectorBodyReadFailure;
};

export function auditAudioRequests(
  requests: readonly AudioRequestEvidence[],
  expectedUrls: readonly string[],
  browserName: string,
) {
  const actual = requests.filter(
    (request) =>
      expectedUrls.includes(request.url) &&
      request.method === "GET" &&
      request.started > 0 &&
      request.routed > request.started &&
      request.continued > request.routed &&
      request.responded > request.routed &&
      request.finished > request.responded &&
      !request.failed &&
      request.responses === 1 &&
      [200, 206].includes(request.status) &&
      !request.headersEmpty &&
      request.audioContentType &&
      request.bodyReadFailure === undefined &&
      request.bodyBytes !== null &&
      request.bodyBytes > 0 &&
      request.bodyVerified,
  );
  const probes = requests.filter(
    (request) =>
      browserName === "webkit" &&
      expectedUrls.includes(request.url) &&
      request.method === "GET" &&
      request.resourceType === "other" &&
      request.started > 0 &&
      request.responded > request.started &&
      request.finished > request.responded &&
      request.routed === 0 &&
      request.continued === 0 &&
      !request.failed &&
      request.responses === 1 &&
      request.status === 0 &&
      request.headersEmpty &&
      ((request.bodyBytes === 0 && request.bodyReadFailure === undefined) ||
        (request.bodyBytes === null &&
          request.bodyReadFailure === "no_resource_data")),
  );
  const unusedActual = new Set(actual);
  let pairedProbes = 0;
  for (const probe of probes) {
    const peer = actual.find(
      (request) =>
        unusedActual.has(request) &&
        request.url === probe.url &&
        request.started > probe.finished,
    );
    if (peer) {
      unusedActual.delete(peer);
      pairedProbes++;
    }
  }
  return {
    violations: requests.length - actual.length - pairedProbes,
    pairedProbes,
    unpairedProbes:
      requests.filter((request) => request.status === 0).length - pairedProbes,
  };
}

export type CapturedAudioBody = {
  path: string;
  bytes: Buffer;
  contentRange?: string;
};

type InspectorBodyReadFailure =
  | "no_resource"
  | "no_resource_data"
  | "evicted"
  | "other";

function inspectorBodyReadFailure(error: unknown): InspectorBodyReadFailure {
  if (!(error instanceof Error)) return "other";
  const reasons = [
    ["no_resource", "Missing resource for given requestId"],
    ["no_resource_data", "Missing content of resource for given requestId"],
    ["evicted", "Resource content was evicted from inspector cache"],
  ] as const;
  for (const [kind, reason] of reasons) {
    const protocol = `Protocol error (Network.getResponseBody): ${reason}`;
    if (
      error.message === protocol ||
      error.message === `response.body: ${protocol}`
    )
      return kind;
  }
  return "other";
}

type Capture = AudioRequestEvidence & {
  body?: CapturedAudioBody;
};
type AuditOptions = {
  origin: string;
  browserName: string;
  expected: () => { first: string; next: string };
  matches: (
    kind: "first" | "next",
    bytes: Buffer,
    contentRange?: string,
  ) => boolean;
  bodies: CapturedAudioBody[];
  pending: Set<Promise<void>>;
};

export class NextEntryAudioAudit {
  private requests = new Map<Request, Capture>();
  private sequence = 0;
  private failures = {
    inspection: 0,
    ledgerOverflow: 0,
    duplicateResponses: 0,
    unexpectedStatus: 0,
    contentType: 0,
    captureLimit: 0,
    bodyTooLarge: 0,
    bodyRead: 0,
    probeBodyRead: 0,
  };

  constructor(
    context: BrowserContext,
    private options: AuditOptions,
  ) {
    context.on("request", (request) =>
      this.inspect(() => {
        const capture = this.capture(request);
        if (capture) capture.started = ++this.sequence;
      }),
    );
    context.on("requestfinished", (request) =>
      this.inspect(() => {
        const capture = this.capture(request);
        if (capture) capture.finished = ++this.sequence;
      }),
    );
    context.on("requestfailed", (request) =>
      this.inspect(() => {
        const capture = this.capture(request);
        if (capture) capture.failed = true;
      }),
    );
    context.on("response", (response) =>
      this.inspect(() => this.response(response)),
    );
  }

  routed(request: Request) {
    this.inspect(() => {
      const capture = this.capture(request);
      if (capture) capture.routed = ++this.sequence;
    });
  }

  continued(request: Request) {
    this.inspect(() => {
      const capture = this.capture(request);
      if (capture) capture.continued = ++this.sequence;
    });
  }

  private inspect(action: () => void) {
    try {
      action();
    } catch {
      this.failures.inspection++;
    }
  }

  private capture(request: Request): Capture | undefined {
    const url = new URL(request.url());
    if (
      url.origin !== this.options.origin ||
      !/^\/api\/v1\/sources\/[0-9a-f-]+\/audio$/.test(url.pathname) ||
      request.method() === "HEAD"
    )
      return;
    const existing = this.requests.get(request);
    if (existing) return existing;
    if (this.requests.size >= 32) {
      this.failures.ledgerOverflow++;
      return;
    }
    const capture: Capture = {
      url: request.url(),
      method: request.method(),
      resourceType: request.resourceType(),
      started: 0,
      routed: 0,
      continued: 0,
      responded: 0,
      finished: 0,
      failed: false,
      responses: 0,
      status: -1,
      headersEmpty: false,
      audioContentType: false,
      bodyBytes: null,
      bodyVerified: false,
    };
    this.requests.set(request, capture);
    return capture;
  }

  private response(response: Response) {
    const capture = this.capture(response.request());
    if (!capture) return;
    capture.responses++;
    if (capture.responses !== 1) {
      this.failures.duplicateResponses++;
      return;
    }
    capture.responded = ++this.sequence;
    capture.status = response.status();
    const headers = response.headers();
    capture.headersEmpty = Object.keys(headers).length === 0;
    capture.audioContentType =
      headers["content-type"]?.startsWith("audio/wav") ?? false;
    const probe =
      this.options.browserName === "webkit" &&
      capture.method === "GET" &&
      capture.resourceType === "other" &&
      capture.status === 0 &&
      capture.headersEmpty &&
      capture.routed === 0;
    if (!probe && ![200, 206].includes(capture.status)) {
      this.failures.unexpectedStatus++;
      return;
    }
    if (!probe && !capture.audioContentType) {
      this.failures.contentType++;
      return;
    }
    const { bodies, pending } = this.options;
    if (bodies.length + pending.size >= 32) {
      this.failures.captureLimit++;
      return;
    }
    const checking = response
      .body()
      .then((bytes) => {
        if (bytes.length > 1_000_000) {
          this.failures.bodyTooLarge++;
          return;
        }
        capture.bodyBytes = bytes.length;
        if (probe) return;
        capture.body = {
          path: new URL(capture.url).pathname,
          bytes,
          contentRange: headers["content-range"],
        };
        bodies.push(capture.body);
      })
      .catch((error: unknown) => {
        capture.bodyReadFailure = inspectorBodyReadFailure(error);
        this.failures.bodyRead++;
        if (probe) this.failures.probeBodyRead++;
      })
      .finally(() => pending.delete(checking));
    pending.add(checking);
  }

  private evidence() {
    const expected = Object.entries(this.options.expected())
      .filter(([, path]) => path !== "")
      .map(([kind, path]) => ({
        kind: kind as "first" | "next",
        url: new URL(path, this.options.origin).href,
      }));
    const requests = [...this.requests.values()].map((capture) => {
      const source = expected.find((source) => source.url === capture.url);
      return {
        ...capture,
        bodyVerified:
          source !== undefined &&
          capture.body !== undefined &&
          this.options.matches(
            source.kind,
            capture.body.bytes,
            capture.body.contentRange,
          ),
      };
    });
    return { expected, requests };
  }

  check() {
    const { expected, requests } = this.evidence();
    const result = auditAudioRequests(
      requests,
      expected.map((source) => source.url),
      this.options.browserName,
    );
    return {
      ...result,
      violations:
        result.violations +
        this.failures.inspection +
        this.failures.ledgerOverflow +
        this.failures.captureLimit +
        this.failures.bodyTooLarge,
    };
  }

  diagnostics() {
    const { expected, requests } = this.evidence();
    return {
      ...this.check(),
      ...this.failures,
      requests: requests.map((request) => ({
        source:
          expected.find((source) => source.url === request.url)?.kind ??
          "other",
        get: request.method === "GET",
        other: request.resourceType === "other",
        routed: request.routed > 0,
        continued: request.continued > 0,
        finished: request.finished > request.responded,
        failed: request.failed,
        responses: request.responses,
        status: request.status,
        headersEmpty: request.headersEmpty,
        audioContentType: request.audioContentType,
        bodyBytes: request.bodyBytes,
        bodyVerified: request.bodyVerified,
        bodyReadFailure: request.bodyReadFailure ?? "none",
      })),
    };
  }
}
