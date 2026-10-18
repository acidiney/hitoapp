// @vitest-environment jsdom
import { createElement, type AnchorHTMLAttributes } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import Shell from "./Shell";
import AlbumPage from "./pages/AlbumPage";
import type { AlbumPageProps } from "./types";
import type { components } from "./generated";

type ReleaseArtworkFetch = components["schemas"]["ReleaseArtworkFetch"];

const page = vi.hoisted(() => ({
  component: "AlbumPage",
  url: "/albums/verified-album",
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
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

test("album covers retrieve exact release artwork, retry failures and reject an abandoned account response", async () => {
  vi.useFakeTimers();
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const release = "6e7cce4d-73ba-402b-bd58-e9eb3716f951";
  const artwork = `/api/v1/releases/${release}/artwork`;
  const availability = `${artwork}/availability`;
  const enqueue = `${artwork}/fetch`;
  const firstJob: ReleaseArtworkFetch = {
    id: "d0805cf8-7738-49c5-bea6-b9af2ae828c0",
    release_id: release,
    status: "queued",
    attempts: 0,
    error: null,
    created_at: 1,
    updated_at: 1,
  };
  const retryJob: ReleaseArtworkFetch = {
    ...firstJob,
    id: "8d783cec-31c8-4636-8c95-8e2199fbcb2c",
    created_at: 3,
    updated_at: 3,
  };
  const firstStatus = `/api/v1/release-artwork-fetches/${firstJob.id}`;
  const retryStatus = `/api/v1/release-artwork-fetches/${retryJob.id}`;
  let resolveOldStatus!: (response: Response) => void;
  const oldStatus = new Promise<Response>((resolve) => {
    resolveOldStatus = resolve;
  });
  let enqueues = 0;
  const accountFor = (options?: RequestInit) =>
    new Headers(options?.headers).get("x-hito-account");
  const ready = {
    ...retryJob,
    status: "ready",
    attempts: 1,
    updated_at: 4,
  } satisfies ReleaseArtworkFetch;
  const fetch = vi.fn((path: string, options?: RequestInit) => {
    const account = accountFor(options);
    if (path === availability && options?.method === "GET")
      return Promise.resolve(Response.json({ can_fetch: account === "alice" }));
    if (account === "alice" && path === enqueue && options?.method === "POST") {
      enqueues++;
      return Promise.resolve(
        Response.json(
          enqueues === 1 ? firstJob : enqueues === 2 ? retryJob : ready,
          { status: 202 },
        ),
      );
    }
    if (
      account === "alice" &&
      path === firstStatus &&
      options?.method === "GET"
    )
      return Promise.resolve(
        Response.json({
          ...firstJob,
          status: "failed",
          attempts: 3,
          error: "not_available",
          updated_at: 2,
        } satisfies ReleaseArtworkFetch),
      );
    if (
      account === "alice" &&
      path === retryStatus &&
      options?.method === "GET"
    )
      return oldStatus;
    throw new Error("Unexpected request outside the release artwork workflow");
  });
  vi.stubGlobal("fetch", fetch);
  const view = () =>
    createElement(Shell, {
      children: createElement(AlbumPage, {
        ...page.props,
        detail: {
          release: {
            id: release,
            library_id: "personal",
            title: "My retained album",
            artist: null,
            artist_id: null,
            track_count: 0,
            duration_ms: 0,
            artwork_recording_id: null,
          },
          tracks: { items: [], total: 0, offset: 0, limit: 60 },
        },
      } satisfies AlbumPageProps),
    });
  const { container, rerender } = render(view());
  const cover = () => container.querySelector(`img[src="${artwork}"]`);
  expect(cover()).toBeTruthy();
  expect(fetch).not.toHaveBeenCalled();

  await act(async () => {
    fireEvent.error(cover()!);
  });
  expect(fetch.mock.calls.map(([path]) => path)).toEqual([
    availability,
    enqueue,
  ]);
  expect(cover()).toBeNull();
  await act(async () => {
    await vi.advanceTimersToNextTimerAsync();
  });
  const retry = screen.getByRole("button", { name: "Retry artwork" });
  expect((retry as HTMLButtonElement).disabled).toBe(false);
  expect(screen.getByRole("status").textContent).toContain(
    "Artwork could not be retrieved",
  );
  await act(async () => {
    await vi.advanceTimersByTimeAsync(10_000);
  });
  expect(enqueues).toBe(1);
  await act(async () => {
    fireEvent.click(retry);
  });
  expect(enqueues).toBe(2);
  await act(async () => {
    await vi.advanceTimersToNextTimerAsync();
  });
  expect(fetch.mock.calls.map(([path]) => path)).toEqual([
    availability,
    enqueue,
    firstStatus,
    availability,
    enqueue,
    retryStatus,
  ]);
  const abandoned = fetch.mock.calls.slice(3);
  expect(
    abandoned.every(
      ([, options]) => options?.signal && !options.signal.aborted,
    ),
  ).toBe(true);

  page.props.user = { id: "bob", username: "Bob", admin: false };
  rerender(view());
  expect(abandoned.every(([, options]) => options?.signal?.aborted)).toBe(true);
  expect(cover()).toBeTruthy();
  await act(async () => {
    fireEvent.error(cover()!);
  });
  expect(cover()).toBeNull();
  expect(
    fetch.mock.calls
      .filter(([, options]) => accountFor(options) === "bob")
      .map(([path]) => path),
  ).toEqual([availability]);
  await act(async () => {
    resolveOldStatus(Response.json(ready));
  });
  expect(cover()).toBeNull();

  page.props.user = { id: "alice", username: "Alice", admin: false };
  rerender(view());
  expect(cover()).toBeTruthy();
  await act(async () => {
    fireEvent.error(cover()!);
  });
  expect(enqueues).toBe(3);
  expect(cover()?.getAttribute("src")).toBe(artwork);
  expect(screen.queryByRole("button", { name: "Retry artwork" })).toBeNull();
  expect(fetch.mock.calls.map(([path]) => path)).toEqual([
    availability,
    enqueue,
    firstStatus,
    availability,
    enqueue,
    retryStatus,
    availability,
    availability,
    enqueue,
  ]);
  for (const [path, options] of fetch.mock.calls) {
    expect(path).not.toContain("/recordings/");
    expect(options?.credentials).toBe("same-origin");
    expect(options?.headers).toEqual(
      expect.objectContaining({ "x-hito-request": "1" }),
    );
    expect(options?.signal).toBeInstanceOf(AbortSignal);
    expect(options?.body).toBeUndefined();
  }
  expect(vi.getTimerCount()).toBe(0);
});
