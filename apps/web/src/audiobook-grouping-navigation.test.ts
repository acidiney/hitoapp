// @vitest-environment jsdom
import { createElement, type ReactNode } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { router } from "@inertiajs/react";
import AudiobookIndexPage from "./pages/AudiobookIndexPage";
import { audiobookGrouping, heldAudiobookBody } from "./fixtures/audiobooks";
import type {
  AudiobookDetail,
  AudiobookIndexPageProps,
  AudiobookPageProps,
  SyncBatch,
} from "./types";

type NavigationOptions = NonNullable<Parameters<typeof router.visit>[1]>;
type NavigationVisit = Parameters<
  NonNullable<NavigationOptions["onFinish"]>
>[0];
type NavigationPage = Parameters<
  NonNullable<NavigationOptions["onSuccess"]>
>[0];

const boundaries = vi.hoisted(() => ({
  player: vi.fn(() => {
    throw new Error("Grouping navigation must not open playback");
  }),
  visit: vi.fn(),
  reload: vi.fn(),
}));
vi.mock("./player/PlaybackProvider", () => ({ usePlayer: boundaries.player }));
vi.mock("@inertiajs/react", () => ({
  router: { visit: boundaries.visit, reload: boundaries.reload },
  Link: ({
    children,
    href,
    ...attributes
  }: {
    children: ReactNode;
    href: string;
  }) => createElement("a", { href, ...attributes }, children),
}));

const request = vi.fn<typeof globalThis.fetch>();
const audio = vi.fn();
beforeEach(() => {
  vi.clearAllMocks();
  vi.spyOn(console, "info").mockImplementation(() => {});
  vi.spyOn(console, "warn").mockImplementation(() => {});
  vi.spyOn(console, "debug").mockImplementation(() => {});
  localStorage.clear();
  audio.mockImplementation(() => {
    throw new Error("Grouping navigation must not create audio");
  });
  vi.stubGlobal("Audio", audio);
  vi.stubGlobal("fetch", request);
});
afterEach(() => {
  cleanup();
  try {
    expect(boundaries.player).not.toHaveBeenCalled();
    expect(audio).not.toHaveBeenCalled();
    expect(boundaries.reload).not.toHaveBeenCalled();
    expect(document.querySelector("audio")).toBeNull();
  } finally {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
    localStorage.clear();
  }
});

test("a failed book navigation recovers through an authoritative read without repeating its accepted mutation", async () => {
  const { index, playlist, confirmed } = audiobookGrouping();
  const held = heldAudiobookBody(confirmed);
  const writes: string[] = [];
  let reads = 0;
  request.mockImplementation(async (input, options) => {
    expect(new Headers(options?.headers).get("x-hito-account")).toBe(
      index.user.id,
    );
    if (input === "/api/v1/sync" && options?.method === "POST") {
      writes.push(String(options.body));
      const batch = JSON.parse(String(options.body)) as SyncBatch;
      return Response.json({
        acknowledged: batch.operations.map((operation) => operation.id),
      });
    }
    if (
      input === `/api/v1/audiobooks/${playlist.id}` &&
      options?.method === "GET"
    ) {
      reads += 1;
      return reads === 1 ? Response.json(confirmed) : held.response;
    }
    throw new Error("Unexpected failed audiobook navigation request");
  });
  let view: ReturnType<typeof render> | undefined;
  try {
    view = render(createElement(AudiobookIndexPage, index));
    expect(request).not.toHaveBeenCalled();
    submit(playlist.id);
    await waitFor(() => expect(boundaries.visit).toHaveBeenCalledTimes(1));
    const first = navigation(0);
    const rawError = "Owned private navigation diagnostics";
    await act(async () => {
      expect(first.onNetworkError?.(new Error(rawError))).toBe(false);
      first.onFinish?.(visit(first, playlist.id));
    });
    const error = await screen.findByRole("alert");
    expect(error.textContent?.includes(rawError)).toBe(false);
    expect(writes).toHaveLength(1);
    expect(request).toHaveBeenCalledTimes(2);
    fireEvent.click(screen.getByRole("button", { name: "Check audiobook" }));
    await waitFor(() => expect(held.response.bodyUsed).toBe(true));
    expect(writes).toHaveLength(1);
    expect(request).toHaveBeenCalledTimes(3);
    expect(boundaries.visit).toHaveBeenCalledTimes(1);
    await act(async () => held.release());
    await waitFor(() => expect(boundaries.visit).toHaveBeenCalledTimes(2));
    expect(vi.mocked(router.visit).mock.calls.map(([path]) => path)).toEqual([
      `/audiobooks/${playlist.id}`,
      `/audiobooks/${playlist.id}`,
    ]);
    const second = navigation(1);
    await act(async () => {
      await second.onSuccess?.(page(index, confirmed));
      second.onFinish?.(visit(second, playlist.id, true));
    });
    expect(screen.queryByRole("alert")).toBeNull();
    expect(
      request.mock.calls.map(([path, options]) => [path, options?.method]),
    ).toEqual([
      ["/api/v1/sync", "POST"],
      [`/api/v1/audiobooks/${playlist.id}`, "GET"],
      [`/api/v1/audiobooks/${playlist.id}`, "GET"],
    ]);
    const output = JSON.stringify([
      vi.mocked(console.info).mock.calls,
      vi.mocked(console.warn).mock.calls,
    ]);
    expect(output.includes(rawError)).toBe(false);
  } finally {
    view?.unmount();
    await act(async () => held.dispose());
  }
});

test("a cancelled book navigation can recover while late old-account callbacks cannot affect a new account", async () => {
  const alice = audiobookGrouping();
  const bobPlaylist = {
    id: "93cd4519-2eb9-48a8-9157-2be45d1e7d2a",
    name: "Bob navigation selection",
    kind: "local",
  };
  const bob: AudiobookIndexPageProps = {
    ...alice.index,
    user: { id: "bob", username: "Bob", admin: false },
    books: [],
    playlists: [bobPlaylist],
    grouping_playlists: [bobPlaylist],
  };
  const writes: SyncBatch[] = [];
  request.mockImplementation(async (input, options) => {
    expect(new Headers(options?.headers).get("x-hito-account")).toBe(
      alice.index.user.id,
    );
    if (input === "/api/v1/sync" && options?.method === "POST") {
      const batch = JSON.parse(String(options.body)) as SyncBatch;
      writes.push(batch);
      return Response.json({
        acknowledged: batch.operations.map((operation) => operation.id),
      });
    }
    if (
      input === `/api/v1/audiobooks/${alice.playlist.id}` &&
      options?.method === "GET"
    )
      return Response.json(alice.confirmed);
    throw new Error("Unexpected cancelled audiobook navigation request");
  });
  let view: ReturnType<typeof render> | undefined;
  try {
    view = render(createElement(AudiobookIndexPage, alice.index));
    expect(request).not.toHaveBeenCalled();
    submit(alice.playlist.id);
    await waitFor(() => expect(boundaries.visit).toHaveBeenCalledTimes(1));
    const first = navigation(0);
    await act(async () => {
      first.onCancel?.();
      first.onFinish?.({ ...visit(first, alice.playlist.id), cancelled: true });
    });
    await screen.findByRole("alert");
    fireEvent.click(screen.getByRole("button", { name: "Check audiobook" }));
    await waitFor(() => expect(boundaries.visit).toHaveBeenCalledTimes(2));
    expect(writes).toHaveLength(1);
    expect(request).toHaveBeenCalledTimes(3);
    const late = navigation(1);
    const cancel = vi.fn();
    late.onCancelToken?.({ cancel });
    view.rerender(createElement(AudiobookIndexPage, bob));
    expect(cancel).toHaveBeenCalledTimes(1);
    expect(
      (screen.getByRole("combobox", { name: "Playlist" }) as HTMLSelectElement)
        .value,
    ).toBe("");
    expect(screen.queryByText(alice.playlist.name, { exact: true })).toBeNull();
    expect(
      screen.queryByRole("link", { name: alice.index.books[0].name }),
    ).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByRole("status")).toBeNull();
    expect(late.onBefore?.(visit(late, alice.playlist.id))).toBe(false);
    await act(async () => {
      late.onCancel?.();
      late.onNetworkError?.(new Error("Obsolete navigation diagnostics"));
      await late.onSuccess?.(page(alice.index, alice.confirmed));
      late.onFinish?.(visit(late, alice.playlist.id));
    });
    expect(boundaries.visit).toHaveBeenCalledTimes(2);
    expect(request).toHaveBeenCalledTimes(3);
    expect(writes).toHaveLength(1);
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByText(alice.playlist.name, { exact: true })).toBeNull();
    const select = screen.getByRole("combobox", {
      name: "Playlist",
    }) as HTMLSelectElement;
    fireEvent.change(select, { target: { value: bobPlaylist.id } });
    expect(
      (
        screen.getByRole("button", {
          name: "Add audiobook",
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(false);
    expect(request).toHaveBeenCalledTimes(3);
  } finally {
    view?.unmount();
  }
});

function submit(playlist: string) {
  fireEvent.change(screen.getByRole("combobox", { name: "Playlist" }), {
    target: { value: playlist },
  });
  fireEvent.click(screen.getByRole("button", { name: "Add audiobook" }));
}

function navigation(index: number): NavigationOptions {
  const options = vi.mocked(router.visit).mock.calls[index]?.[1];
  if (!options)
    throw new Error(
      "Audiobook navigation must own its failure, cancel and finish callbacks",
    );
  expect(typeof options.onNetworkError).toBe("function");
  expect(typeof options.onCancel).toBe("function");
  expect(typeof options.onFinish).toBe("function");
  return options;
}

function page(
  index: AudiobookIndexPageProps,
  detail: AudiobookDetail,
): NavigationPage {
  const props: AudiobookPageProps = {
    errors: index.errors,
    native_playback: index.native_playback,
    user: index.user,
    libraries: index.libraries,
    playlists: index.playlists,
    detail,
  };
  return {
    component: "AudiobookPage",
    url: `/audiobooks/${props.detail.playlist.id}`,
    props: { ...props, errors: {} },
    version: null,
    rescuedProps: [],
    flash: {},
    rememberedState: {},
  };
}

function visit(
  options: NavigationOptions,
  playlist: string,
  completed = false,
): NavigationVisit {
  const noop = () => {};
  return {
    id: "owned-audiobook-navigation",
    url: new URL(`http://localhost/audiobooks/${playlist}`),
    method: "get",
    data: {},
    completed,
    cancelled: false,
    interrupted: false,
    replace: false,
    preserveScroll: false,
    preserveState: false,
    only: options.only ?? [],
    except: [],
    headers: {},
    errorBag: null,
    forceFormData: false,
    queryStringArrayFormat: "brackets",
    async: false,
    showProgress: false,
    prefetch: false,
    fresh: false,
    reset: [],
    preserveUrl: false,
    preserveErrors: false,
    invalidateCacheTags: [],
    viewTransition: false,
    component: null,
    pageProps: null,
    cached: false,
    onCancelToken: options.onCancelToken ?? noop,
    onBefore: options.onBefore ?? noop,
    onBeforeUpdate: options.onBeforeUpdate ?? noop,
    onStart: options.onStart ?? noop,
    onProgress: options.onProgress ?? noop,
    onFinish: options.onFinish ?? noop,
    onCancel: options.onCancel ?? noop,
    onSuccess: options.onSuccess ?? noop,
    onError: options.onError ?? noop,
    onHttpException: options.onHttpException ?? noop,
    onNetworkError: options.onNetworkError ?? noop,
    onFlash: options.onFlash ?? noop,
    onPrefetched: options.onPrefetched ?? noop,
    onPrefetching: options.onPrefetching ?? noop,
  };
}
