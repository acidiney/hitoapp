import { isDeepStrictEqual } from "node:util";
import {
  expect,
  type BrowserContext,
  type Request,
  type Response,
} from "@playwright/test";

type CompletionState = "pending" | "finished" | "failed";
type CompletionSnapshot = {
  request: CompletionState;
  response: CompletionState;
  requestFinishedEvent: boolean;
  requestFailedEvent: boolean;
  responseEnd: boolean;
  failure: "none" | "abort" | "other";
};
type NegativeCheck = {
  route: "artwork" | "logout";
  stage: "headers" | "body" | "complete";
  noStore?: boolean;
  json?: boolean;
  requestUuid?: boolean;
  declaredBytes?: number | null;
  bodyBytes?: number | null;
  canonical?: boolean;
  bodyObservation?: "canonical" | "unavailable" | "unobserved";
  bodyReason?: "consumption_sensitive";
  completionAtDeadline?: CompletionSnapshot & { shortDeadlineElapsed: boolean };
  completionBeforeCleanup?: CompletionSnapshot;
};

export function createNegativeEvidence(context: BrowserContext) {
  const counts = {
    artwork404: 0,
    logout401: 0,
    bodyAvailable: 0,
    bodyUnavailable: 0,
    bodyUnobserved: 0,
  };
  const checks: NegativeCheck[] = [];
  let matchedLogout: Request | undefined;
  let requestState: CompletionState = "pending";
  let requestFinishedEvent = false;
  let requestFailedEvent = false;
  let closed = false;
  const finished = (request: Request) => {
    if (!closed && request === matchedLogout) {
      requestState = "finished";
      requestFinishedEvent = true;
    }
  };
  const failed = (request: Request) => {
    if (!closed && request === matchedLogout) {
      requestState = "failed";
      requestFailedEvent = true;
    }
  };
  context.on("requestfinished", finished);
  context.on("requestfailed", failed);
  function observeCompletion(response: Response, check: NegativeCheck) {
    const request = response.request();
    let responseState: CompletionState = "pending";
    let active = true;
    void response.finished().then(
      (error) => {
        if (active && !closed)
          responseState = error === null ? "finished" : "failed";
      },
      () => {
        if (active && !closed) responseState = "failed";
      },
    );
    const snapshot = (): CompletionSnapshot => {
      const failure = request.failure();
      const end = request.timing().responseEnd;
      return {
        request: requestState,
        response: responseState,
        requestFinishedEvent,
        requestFailedEvent,
        responseEnd: Number.isFinite(end) && end >= 0,
        failure:
          failure === null
            ? "none"
            : /abort|cancel|closed|interrupted/i.test(failure.errorText)
              ? "abort"
              : "other",
      };
    };
    const timer = setTimeout(() => {
      check.completionAtDeadline = {
        ...snapshot(),
        shortDeadlineElapsed: true,
      };
    }, 1000);
    let final: CompletionSnapshot | undefined;
    return () => {
      if (final) return final;
      clearTimeout(timer);
      final = snapshot();
      check.completionAtDeadline ??= { ...final, shortDeadlineElapsed: false };
      check.completionBeforeCleanup = final;
      active = false;
      return final;
    };
  }
  async function read(
    response: Response,
    route: "/api/v1/recordings/{id}/artwork" | "/api/v1/me",
  ) {
    const check: NegativeCheck = {
      route: response.status() === 404 ? "artwork" : "logout",
      stage: "headers",
    };
    checks.push(check);
    if (route === "/api/v1/recordings/{id}/artwork") counts.artwork404++;
    else {
      const url = new URL(response.url());
      expect(
        response.status() === 401 &&
          response.request().method() === "GET" &&
          url.pathname === "/api/v1/me" &&
          !url.search &&
          !url.hash &&
          counts.logout401 === 0,
      ).toBe(true);
      counts.logout401++;
      matchedLogout = response.request();
    }
    const headers = await response.allHeaders();
    const length = Number(headers["content-length"]);
    check.noStore = !!headers["cache-control"]?.includes("no-store");
    check.json = !!headers["content-type"]?.includes("application/json");
    check.requestUuid =
      /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(
        headers["x-request-id"] ?? "",
      );
    check.declaredBytes = Number.isSafeInteger(length) ? length : null;
    expect(
      headers["cache-control"]?.includes("no-store") &&
        headers["content-type"]?.includes("application/json") &&
        /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(
          headers["x-request-id"] ?? "",
        ) &&
        Number.isSafeInteger(length) &&
        length > 0 &&
        length <= 256 &&
        (route !== "/api/v1/me" || length === 31),
    ).toBe(true);
    check.stage = "body";
    const stopCompletion: () => CompletionSnapshot | undefined =
      route === "/api/v1/me"
        ? observeCompletion(response, check)
        : () => undefined;
    const pendingBody = Symbol("owned pending logout body");
    let timer: ReturnType<typeof setTimeout> | undefined;
    try {
      const bytes = await Promise.race([
        response.body(),
        new Promise<typeof pendingBody>((resolve, reject) => {
          timer = setTimeout(() => {
            if (route === "/api/v1/me") resolve(pendingBody);
            else reject(new Error("Owned negative reply deadline"));
          }, 5000);
        }),
      ]);
      if (bytes === pendingBody) {
        const completion = stopCompletion();
        expect(
          route === "/api/v1/me" &&
            completion?.request === "pending" &&
            completion.response === "pending" &&
            !completion.requestFinishedEvent &&
            !completion.requestFailedEvent &&
            !completion.responseEnd &&
            completion.failure === "none",
        ).toBe(true);
        check.bodyBytes = null;
        check.bodyObservation = "unobserved";
        check.bodyReason = "consumption_sensitive";
        counts.bodyUnavailable++;
        counts.bodyUnobserved++;
      } else {
        check.bodyBytes = bytes.length;
        try {
          check.canonical = isDeepStrictEqual(
            JSON.parse(bytes.toString("utf8")),
            {
              error:
                response.status() === 404
                  ? "Resource not found"
                  : "Sign in to continue",
            },
          );
        } catch {
          check.canonical = false;
        }
        expect(
          bytes.length === length &&
            (route !== "/api/v1/recordings/{id}/artwork" || length === 30) &&
            isDeepStrictEqual(JSON.parse(bytes.toString("utf8")), {
              error:
                response.status() === 404
                  ? "Resource not found"
                  : "Sign in to continue",
            }),
        ).toBe(true);
        check.bodyObservation = "canonical";
        counts.bodyAvailable++;
      }
    } finally {
      clearTimeout(timer);
      stopCompletion();
    }
    check.stage = "complete";
    return {
      url: response.url(),
      request_id: headers["x-request-id"],
      status: response.status(),
      route,
    };
  }

  return {
    counts,
    checks,
    read,
    close() {
      closed = true;
      context.off("requestfinished", finished);
      context.off("requestfailed", failed);
    },
  };
}
