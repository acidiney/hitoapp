import { vi } from "vitest";
import { artistAssociation, reviewedArtistProfile } from "./artistReview";
import { firstDiscographyPage, lastDiscographyPage } from "./artistDiscography";

type Stage = "associations" | "profile" | "discography";
type Reply = (signal: AbortSignal) => Promise<Response> | Response;

export function artistGroupHttp() {
  const base = `/api/v1/recordings/${artistAssociation.recording_id}/artist-associations`;
  const paths = {
    associations: base,
    profile: `${base}/${artistAssociation.id}/profile`,
    discography: `${base}/${artistAssociation.id}/discography`,
  };
  const replies: Record<Stage, Reply[]> = {
    associations: [],
    profile: [],
    discography: [],
  };
  const calls: Record<Stage, number> = {
    associations: 0,
    profile: 0,
    discography: 0,
  };
  const library = {
    requests: [] as string[][],
    replies: [] as Array<(ids: string[]) => Response>,
  };
  const cleanup: Array<() => void> = [];
  const responses: Response[] = [];
  const response = (value: unknown, status = 200) => {
    const result = Response.json(value, { status });
    responses.push(result);
    return result;
  };
  const fetch = vi.fn(async (path: string, options?: RequestInit) => {
    if (path === `${paths.discography}/library` && options?.signal) {
      const ids = (
        JSON.parse(options.body as string) as {
          release_group_ids: string[];
        }
      ).release_group_ids;
      library.requests.push(ids);
      const next = library.replies.shift();
      if (next) return next(ids);
      return response({
        association_id: artistAssociation.id,
        groups: ids.map((id) => ({ release_group_id: id, albums: [] })),
      });
    }
    const stage = (Object.keys(paths) as Stage[]).find(
      (name) => paths[name] === path,
    );
    if (
      !stage ||
      !options?.signal ||
      options.method !== (stage === "discography" ? "POST" : "GET")
    )
      throw new Error("Unexpected request outside chosen artist browsing");
    calls[stage]++;
    const next = replies[stage].shift();
    if (next) return next(options.signal);
    return response(
      stage === "associations"
        ? [artistAssociation]
        : stage === "profile"
          ? reviewedArtistProfile
          : calls.discography === 1
            ? firstDiscographyPage
            : lastDiscographyPage,
    );
  });
  vi.stubGlobal("fetch", fetch);
  return {
    fetch,
    calls,
    libraryRequests: library.requests,
    replyLibrary(value: unknown | ((ids: string[]) => unknown), status = 200) {
      library.replies.push((ids) =>
        response(typeof value === "function" ? value(ids) : value, status),
      );
    },
    responses,
    reply(stage: Stage, value: unknown, status = 200) {
      replies[stage].push(() => response(value, status));
    },
    hold(stage: Stage, mode: "headers" | "body", value: unknown, status = 200) {
      const bytes = new TextEncoder().encode(JSON.stringify(value));
      if (bytes.length > 65_536)
        throw new Error("Owned response exceeds limit");
      let enter!: () => void;
      const entered = new Promise<void>((resolve) => {
        enter = resolve;
      });
      let releaseHeaders!: (value: Response) => void;
      const headers = new Promise<Response>((resolve) => {
        releaseHeaders = resolve;
      });
      let body!: ReadableStreamDefaultController<Uint8Array>;
      let released = false;
      const result =
        mode === "headers"
          ? response(value, status)
          : new Response(
              new ReadableStream<Uint8Array>({
                start(controller) {
                  body = controller;
                  controller.enqueue(bytes.slice(0, 1));
                },
                pull() {
                  enter();
                },
                cancel() {
                  released = true;
                },
              }),
              { status, headers: { "content-type": "application/json" } },
            );
      let signal: AbortSignal | null = null;
      let aborts = 0;
      const aborted = () => {
        aborts++;
      };
      const release = () => {
        if (released) return;
        released = true;
        if (mode === "headers") releaseHeaders(result);
        else {
          body.enqueue(bytes.slice(1));
          body.close();
        }
      };
      replies[stage].push((current) => {
        signal = current;
        signal.addEventListener("abort", aborted, { once: true });
        if (mode === "headers") {
          enter();
          return headers;
        }
        return result;
      });
      cleanup.push(() => {
        release();
        signal?.removeEventListener("abort", aborted);
      });
      return { entered, release, signal: () => signal, aborts: () => aborts };
    },
    close() {
      for (const finish of cleanup) finish();
    },
  };
}
