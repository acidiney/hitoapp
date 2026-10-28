import { log } from "./log";

type RequestOptions = {
  action: string;
  method?: "GET" | "POST" | "PUT" | "DELETE";
  body?: unknown;
  account?: string;
  signal?: AbortSignal;
  cache?: RequestCache;
  errorBody?: boolean;
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
  try {
    response = await fetch(`/api/v1${path}`, {
      method: options.method ?? "GET",
      credentials: "same-origin",
      signal: options.signal,
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
    const details = {
      ...fields,
      reason: options.signal?.aborted ? "cancelled" : "network",
      duration_ms: Math.round(performance.now() - started),
    };
    if (options.signal?.aborted) log.debug("http.cancelled", details);
    else log.warn("http.failed", details);
    throw error;
  }
  const result = {
    ...fields,
    status: response.status,
    request_id: response.headers.get("x-request-id") ?? undefined,
    duration_ms: Math.round(performance.now() - started),
  };
  if (response.ok) {
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
  const message =
    data !== null &&
    typeof data === "object" &&
    "error" in data &&
    typeof data.error === "string"
      ? data.error
      : `Hito returned ${response.status}. Try again.`;
  const code =
    data !== null &&
    typeof data === "object" &&
    "code" in data &&
    typeof data.code === "string"
      ? data.code
      : undefined;
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
