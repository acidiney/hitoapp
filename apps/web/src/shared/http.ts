import {
  ConnectionError,
  gatewayMessage,
  reportReachable,
  reportUnreachable,
} from "./connection";
import { log } from "./log";

const defaultTimeoutMs = 90_000;

type RequestOptions = {
  action: string;
  method?: "GET" | "POST" | "PUT" | "DELETE";
  body?: unknown;
  account?: string;
  signal?: AbortSignal;
  cache?: RequestCache;
  errorBody?: boolean;
  timeoutMs?: number;
};

export const lockedEvent = "hito-session-locked";

export class HttpError extends Error {
  constructor(
    readonly status: number,
    message: string,
    readonly code?: string,
    readonly retryAfter?: number,
  ) {
    super(message);
    this.name = "HttpError";
  }
}

export async function request(path: string, options: RequestOptions) {
  const started = performance.now();
  const fields = {
    action: options.action,
    method: options.method ?? "GET",
    account_id: options.account,
  };
  log.debug("http.request", fields);
  let response: Response;
  const limit = new AbortController();
  let timedOut = false;
  const timer = setTimeout(() => {
    timedOut = true;
    limit.abort();
  }, options.timeoutMs ?? defaultTimeoutMs);
  const stopTimer = () => clearTimeout(timer);
  options.signal?.addEventListener("abort", stopTimer, { once: true });
  const signal = options.signal
    ? typeof AbortSignal.any === "function"
      ? AbortSignal.any([options.signal, limit.signal])
      : options.signal
    : limit.signal;
  try {
    response = await fetch(`/api/v1${path}`, {
      method: options.method ?? "GET",
      credentials: "same-origin",
      signal,
      cache: options.cache,
      headers: {
        "content-type": "application/json",
        "x-hito-request": "1",
        ...(options.account ? { "x-hito-account": options.account } : {}),
      },
      body:
        options.body === undefined ? undefined : JSON.stringify(options.body),
    });
  } catch (error) {
    const cancelled = options.signal?.aborted === true;
    const reason = cancelled ? "cancelled" : timedOut ? "timeout" : "network";
    const details = {
      ...fields,
      reason,
      duration_ms: Math.round(performance.now() - started),
    };
    if (cancelled) {
      log.debug("http.cancelled", details);
      throw error;
    }
    log.warn("http.failed", details);
    reportUnreachable(timedOut ? "timeout" : "network");
    throw new ConnectionError(timedOut ? "timeout" : "network");
  } finally {
    clearTimeout(timer);
    options.signal?.removeEventListener("abort", stopTimer);
  }
  const result = {
    ...fields,
    status: response.status,
    request_id: response.headers.get("x-request-id") ?? undefined,
    duration_ms: Math.round(performance.now() - started),
  };
  if (response.ok) {
    reportReachable();
    const accepted =
      options.action === "job.status" ||
      options.action === "downloads.status" ||
      options.action === "source_preparation.status" ||
      options.action === "playlists.follow.status" ||
      options.action === "podcasts.status" ||
      options.action.startsWith("artwork.") ||
      options.action.endsWith("tracks_page") ||
      options.action === "progress.save" ||
      options.action === "session.verify" ||
      options.action === "lock.state" ||
      options.action === "lock.activity" ||
      options.action === "community.playback.report" ||
      options.action.startsWith("native.")
        ? log.debug
        : log.info;
    accepted("http.response", result);
    return response;
  }
  if (response.status === 423) window.dispatchEvent(new Event(lockedEvent));
  if (
    (["downloads.status", "source_preparation.status"].includes(
      options.action,
    ) &&
      response.status === 404) ||
    (options.action === "session.verify" &&
      [401, 423].includes(response.status))
  )
    log.debug("http.response", result);
  else log.warn("http.failed", result);
  let data: unknown = null;
  if (options.errorBody === false) void response.body?.cancel().catch(() => {});
  else data = await response.json().catch(() => null);
  const detail =
    data !== null &&
    typeof data === "object" &&
    "error" in data &&
    typeof data.error === "string"
      ? data.error
      : undefined;
  const code =
    data !== null &&
    typeof data === "object" &&
    "code" in data &&
    typeof data.code === "string"
      ? data.code
      : undefined;
  const gateway =
    [502, 503, 504].includes(response.status) &&
    options.errorBody !== false &&
    detail === undefined;
  if (gateway) reportUnreachable("gateway");
  else reportReachable();
  const message = gateway
    ? gatewayMessage
    : (detail ?? `Hito returned ${response.status}. Try again.`);
  const wait = Number(response.headers.get("retry-after"));
  throw new HttpError(
    response.status,
    message,
    code,
    Number.isFinite(wait) && wait >= 0 ? wait : undefined,
  );
}

export async function requestJson<T>(
  path: string,
  options: RequestOptions,
): Promise<T> {
  return (await request(path, options)).json() as Promise<T>;
}
