import { createElement, Fragment, type AnchorHTMLAttributes } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import Shell from "./Shell";
import Artwork from "./components/Artwork";
import RecordingDetailsPage from "./pages/RecordingDetailsPage";
import { sourceAttachmentPage } from "./fixtures/sourceAttachment";
import type { components } from "./generated";

type ArtworkFetch = components["schemas"]["ArtworkFetch"];

const page = vi.hoisted(() => ({
  component: "CatalogPage",
  url: "/library",
  props: {
    user: { id: "alice", username: "Alice", admin: false },
    errors: {},
    native_playback: false,
    libraries: [],
    playlists: [],
  },
}));

vi.mock("@inertiajs/react", () => ({
  usePage: () => page,
  router: { get: vi.fn(), clearHistory: vi.fn() },
  Link: ({ children, ...props }: AnchorHTMLAttributes<HTMLAnchorElement>) =>
    createElement("a", props, children),
}));

afterEach(() => {
  cleanup();
  page.props.user = { id: "alice", username: "Alice", admin: false };
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((accept) => {
    resolve = accept;
  });
  return { promise, resolve };
}

test("duplicate missing recording covers share retrieval and retry retained artwork when ready", async () => {
  vi.useFakeTimers();
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});

  const recording = "cd53993a-c582-42af-a67b-61280e39d3c2";
  const artwork = `/api/v1/recordings/${recording}/artwork`;
  const availability = `${artwork}/availability`;
  const enqueue = `${artwork}/fetch`;
  const job: ArtworkFetch = {
    id: "9b5d2fa0-23a7-4e08-9da9-88a5839f1252",
    recording_id: recording,
    status: "queued",
    attempts: 0,
    error: null,
    created_at: 1,
    updated_at: 1,
  };
  const status = `/api/v1/artwork-fetches/${job.id}`;
  const running = deferred<Response>();
  const ready = deferred<Response>();
  let statusReads = 0;
  const fetch = vi.fn((path: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    if (path === availability && method === "GET")
      return Promise.resolve(Response.json({ can_fetch: true }));
    if (path === enqueue && method === "POST")
      return Promise.resolve(Response.json(job, { status: 202 }));
    if (path === status && method === "GET") {
      statusReads++;
      if (statusReads === 1) return running.promise;
      if (statusReads === 2) return ready.promise;
    }
    throw new Error("Unexpected request outside the artwork workflow");
  });
  vi.stubGlobal("fetch", fetch);

  const { container } = render(
    createElement(Shell, {
      children: createElement(
        Fragment,
        null,
        createElement(Artwork, { recording }),
        createElement(Artwork, { recording }),
      ),
    }),
  );
  const covers = () =>
    [...container.querySelectorAll("img")].filter(
      (image) => image.getAttribute("src") === artwork,
    );
  const requestsFor = (path: string) =>
    fetch.mock.calls.filter(([requested]) => requested === path);

  expect(covers()).toHaveLength(2);
  expect(fetch).not.toHaveBeenCalled();
  await act(async () => {
    for (const image of covers()) fireEvent.error(image);
  });
  expect(requestsFor(availability)).toHaveLength(1);
  expect(requestsFor(enqueue)).toHaveLength(1);
  expect(covers()).toHaveLength(0);

  if (statusReads === 0)
    await act(async () => {
      await vi.advanceTimersToNextTimerAsync();
    });
  expect(statusReads).toBe(1);
  await act(async () => {
    running.resolve(
      Response.json({
        ...job,
        status: "running",
        attempts: 1,
        updated_at: 2,
      } satisfies ArtworkFetch),
    );
  });
  expect(covers()).toHaveLength(0);
  await act(async () => {
    await vi.advanceTimersToNextTimerAsync();
  });
  expect(statusReads).toBe(2);
  await act(async () => {
    ready.resolve(
      Response.json({
        ...job,
        status: "ready",
        attempts: 1,
        updated_at: 3,
      } satisfies ArtworkFetch),
    );
  });
  expect(covers()).toHaveLength(2);
  for (const image of covers()) fireEvent.load(image);

  expect(requestsFor(availability)).toHaveLength(1);
  expect(requestsFor(enqueue)).toHaveLength(1);
  expect(requestsFor(status)).toHaveLength(2);
  for (const [, options] of fetch.mock.calls) {
    expect(options?.credentials).toBe("same-origin");
    expect(options?.headers).toEqual(
      expect.objectContaining({
        "x-hito-account": "alice",
        "x-hito-request": "1",
      }),
    );
    expect(options?.signal).toBeInstanceOf(AbortSignal);
    expect(options?.body).toBeUndefined();
  }
  await act(async () => {
    await vi.advanceTimersByTimeAsync(60_000);
  });
  expect(fetch).toHaveBeenCalledTimes(4);
  expect(vi.getTimerCount()).toBe(0);
  expect(covers()).toHaveLength(2);
});

test("recording details retries failed artwork only after an explicit action", async () => {
  vi.useFakeTimers();
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const recording = sourceAttachmentPage.detail.recording_id;
  const artwork = `/api/v1/recordings/${recording}/artwork`;
  const availability = `${artwork}/availability`;
  const enqueue = `${artwork}/fetch`;
  const firstJob: ArtworkFetch = {
    id: "ed7aa776-a6e3-4969-b839-c280c5e47cf1",
    recording_id: recording,
    status: "queued",
    attempts: 0,
    error: null,
    created_at: 1,
    updated_at: 1,
  };
  const retryJob: ArtworkFetch = {
    ...firstJob,
    id: "13df5dc7-b9df-42f0-8956-48473665f231",
    created_at: 3,
    updated_at: 3,
  };
  const firstStatus = `/api/v1/artwork-fetches/${firstJob.id}`;
  const retryStatus = `/api/v1/artwork-fetches/${retryJob.id}`;
  const downloadStatus = `/api/v1/recordings/${recording}/download`;
  const sourceChoice = `/api/v1/recordings/${recording}/source-choice`;
  let enqueues = 0;
  const fetch = vi.fn(async (path: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    if (path === sourceChoice && method === "GET")
      return Response.json({ error: "Not found" }, { status: 404 });
    if (path === downloadStatus && method === "GET")
      return Response.json({ error: "Not found" }, { status: 404 });
    if (path === availability && method === "GET")
      return Response.json({ can_fetch: true });
    if (path === enqueue && method === "POST") {
      enqueues++;
      return Response.json(enqueues === 1 ? firstJob : retryJob, {
        status: 202,
      });
    }
    if (path === firstStatus && method === "GET")
      return Response.json({
        ...firstJob,
        status: "failed",
        attempts: 3,
        error: "attempts_exhausted",
        updated_at: 2,
      } satisfies ArtworkFetch);
    if (path === retryStatus && method === "GET")
      return Response.json({
        ...retryJob,
        status: "ready",
        attempts: 1,
        updated_at: 4,
      } satisfies ArtworkFetch);
    throw new Error("Unexpected request outside the artwork retry workflow");
  });
  const requests = () =>
    fetch.mock.calls.filter(
      ([path, options]) =>
        !(
          (path === downloadStatus || path === sourceChoice) &&
          (options?.method ?? "GET") === "GET"
        ),
    );
  vi.stubGlobal("fetch", fetch);
  const { container } = render(
    createElement(Shell, {
      children: createElement(RecordingDetailsPage, sourceAttachmentPage),
    }),
  );
  const cover = () => container.querySelector(`img[src="${artwork}"]`);

  expect(screen.queryByRole("button", { name: "Retry artwork" })).toBeNull();
  expect(cover()).toBeTruthy();
  await act(async () => {
    fireEvent.error(cover()!);
  });
  await act(async () => {
    await vi.advanceTimersToNextTimerAsync();
  });
  const retry = screen.getByRole("button", { name: "Retry artwork" });
  expect((retry as HTMLButtonElement).disabled).toBe(false);
  expect(cover()).toBeNull();
  expect(requests()).toHaveLength(3);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(60_000);
  });
  expect(requests()).toHaveLength(3);

  await act(async () => {
    fireEvent.click(retry);
  });
  expect(cover()?.getAttribute("src")).toBe(artwork);
  await act(async () => {
    await vi.advanceTimersToNextTimerAsync();
  });
  expect(cover()?.getAttribute("src")).toBe(artwork);
  expect(screen.queryByRole("button", { name: "Retry artwork" })).toBeNull();
  expect(requests().map(([path]) => path)).toEqual([
    availability,
    enqueue,
    firstStatus,
    availability,
    enqueue,
    retryStatus,
  ]);
  for (const [, options] of fetch.mock.calls) {
    expect(options?.credentials).toBe("same-origin");
    expect(options?.headers).toEqual(
      expect.objectContaining({ "x-hito-account": "alice" }),
    );
    expect(options?.signal).toBeInstanceOf(AbortSignal);
  }
  const choiceReads = fetch.mock.calls.filter(
    ([path, options]) => path === sourceChoice && options?.method === "GET",
  );
  expect(choiceReads).toHaveLength(1);
  expect(choiceReads[0][1]?.signal?.aborted).toBe(false);
  expect(choiceReads[0][1]?.body).toBeUndefined();
  expect(play).not.toHaveBeenCalled();
  expect(document.querySelector("audio")?.getAttribute("src")).toBeNull();
  expect(vi.getTimerCount()).toBe(0);
});

test("a late artwork result cannot restore covers after an account change", async () => {
  vi.useFakeTimers();
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const recording = "cd53993a-c582-42af-a67b-61280e39d3c2";
  const artwork = `/api/v1/recordings/${recording}/artwork`;
  const availability = `${artwork}/availability`;
  const enqueue = `${artwork}/fetch`;
  const job: ArtworkFetch = {
    id: "cb216a10-f10d-40bf-b7dc-2bfc8600c17d",
    recording_id: recording,
    status: "queued",
    attempts: 0,
    error: null,
    created_at: 1,
    updated_at: 1,
  };
  const status = `/api/v1/artwork-fetches/${job.id}`;
  const oldStatus = deferred<Response>();
  const accountFor = (options?: RequestInit) =>
    new Headers(options?.headers).get("x-hito-account");
  const fetch = vi.fn((path: string, options?: RequestInit) => {
    const account = accountFor(options);
    const method = options?.method ?? "GET";
    if (path === availability && method === "GET")
      return Promise.resolve(Response.json({ can_fetch: account === "alice" }));
    if (account === "alice" && path === enqueue && method === "POST")
      return Promise.resolve(Response.json(job, { status: 202 }));
    if (account === "alice" && path === status && method === "GET")
      return oldStatus.promise;
    throw new Error("Unexpected request outside the account artwork workflow");
  });
  vi.stubGlobal("fetch", fetch);
  const view = () =>
    createElement(Shell, {
      children: createElement(
        Fragment,
        null,
        createElement(Artwork, { recording }),
        createElement(Artwork, { recording }),
      ),
    });
  const { container, rerender } = render(view());
  const covers = () => [...container.querySelectorAll(`img[src="${artwork}"]`)];
  await act(async () => {
    for (const image of covers()) fireEvent.error(image);
  });
  await act(async () => {
    await vi.advanceTimersToNextTimerAsync();
  });
  const oldRequests = fetch.mock.calls.filter(
    ([, options]) => accountFor(options) === "alice",
  );
  expect(oldRequests.map(([path]) => path)).toEqual([
    availability,
    enqueue,
    status,
  ]);
  const oldSignals = oldRequests.map(([, options]) => options?.signal);
  expect(oldSignals.every((signal) => signal && !signal.aborted)).toBe(true);
  expect(covers()).toHaveLength(0);

  page.props.user = { id: "bob", username: "Bob", admin: false };
  rerender(view());
  expect(oldSignals.every((signal) => signal?.aborted)).toBe(true);
  expect(covers()).toHaveLength(2);
  await act(async () => {
    for (const image of covers()) fireEvent.error(image);
  });
  expect(covers()).toHaveLength(0);
  const newRequests = fetch.mock.calls.filter(
    ([, options]) => accountFor(options) === "bob",
  );
  expect(newRequests.map(([path]) => path)).toEqual([availability]);
  expect(newRequests[0][1]?.signal?.aborted).toBe(false);

  await act(async () => {
    oldStatus.resolve(
      Response.json({
        ...job,
        status: "ready",
        attempts: 1,
        updated_at: 2,
      } satisfies ArtworkFetch),
    );
  });
  expect(covers()).toHaveLength(0);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(60_000);
  });
  expect(fetch).toHaveBeenCalledTimes(4);
  expect(covers()).toHaveLength(0);
  expect(vi.getTimerCount()).toBe(0);
});

test("missing covers beyond the admission limit start as earlier artwork completes", async () => {
  vi.useFakeTimers();
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const recordings = Array.from(
    { length: 17 },
    (_, index) =>
      `00000000-0000-4000-8000-${String(index + 1).padStart(12, "0")}`,
  );
  const availability = (recording: string) =>
    `/api/v1/recordings/${recording}/artwork/availability`;
  const firstAvailability = deferred<Response>();
  const secondAvailability = deferred<Response>();
  const firstEnqueue = deferred<Response>();
  const secondEnqueue = deferred<Response>();
  const eligible = new Set<string>();
  const enqueued: string[] = [];
  let pending = 0;
  let peakPending = 0;
  const ready = (recording: string) =>
    Response.json(
      {
        id: recording,
        recording_id: recording,
        status: "ready",
        attempts: 1,
        error: null,
        created_at: 1,
        updated_at: 2,
      } satisfies ArtworkFetch,
      { status: 202 },
    );
  const fetch = vi.fn((path: string, options?: RequestInit) => {
    const route = path.match(
      /^\/api\/v1\/recordings\/([^/]+)\/artwork\/(availability|fetch)$/,
    );
    if (!route || !recordings.includes(route[1]))
      throw new Error(
        "Unexpected request outside the bounded artwork workflow",
      );
    const [, recording, action] = route;
    let response: Promise<Response>;
    if (action === "availability" && options?.method === "GET") {
      eligible.add(recording);
      response =
        recording === recordings[0]
          ? firstAvailability.promise
          : recording === recordings[1]
            ? secondAvailability.promise
            : Promise.resolve(Response.json({ can_fetch: true }));
    } else if (action === "fetch" && options?.method === "POST") {
      enqueued.push(recording);
      response =
        enqueued.length === 1
          ? firstEnqueue.promise
          : enqueued.length === 2
            ? secondEnqueue.promise
            : Promise.resolve(ready(recording));
    } else {
      throw new Error("Unexpected artwork request method");
    }
    pending++;
    peakPending = Math.max(peakPending, pending);
    return response.finally(() => {
      pending--;
    });
  });
  vi.stubGlobal("fetch", fetch);
  const { container } = render(
    createElement(Shell, {
      children: createElement(
        Fragment,
        null,
        ...recordings.map((recording) =>
          createElement(Artwork, { key: recording, recording }),
        ),
      ),
    }),
  );
  const covers = () => [...container.querySelectorAll("main img")];
  expect(covers()).toHaveLength(17);
  await act(async () => {
    for (const image of covers()) fireEvent.error(image);
  });
  expect(fetch.mock.calls.map(([path]) => path)).toEqual([
    availability(recordings[0]),
    availability(recordings[1]),
  ]);
  expect(pending).toBe(2);
  expect(peakPending).toBe(2);
  await act(async () => {
    firstAvailability.resolve(Response.json({ can_fetch: true }));
    secondAvailability.resolve(Response.json({ can_fetch: true }));
  });
  expect(eligible).toEqual(new Set(recordings.slice(0, 16)));
  expect(enqueued).toHaveLength(2);
  expect(covers()).toHaveLength(0);
  expect(pending).toBe(2);
  expect(peakPending).toBe(2);

  await act(async () => {
    firstEnqueue.resolve(ready(enqueued[0]));
  });
  expect(eligible.has(recordings[16])).toBe(true);
  expect(enqueued).toContain(recordings[16]);
  expect(peakPending).toBe(2);
  await act(async () => {
    secondEnqueue.resolve(ready(enqueued[1]));
  });
  expect(enqueued).toHaveLength(17);
  expect(covers()).toHaveLength(17);
  expect(pending).toBe(0);
  expect(peakPending).toBe(2);
  expect(vi.getTimerCount()).toBe(0);
});
