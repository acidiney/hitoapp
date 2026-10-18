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
import AudiobookIndexPage from "./pages/AudiobookIndexPage";
import { audiobookGrouping, heldAudiobookBody } from "./fixtures/audiobooks";
import type {
  AudiobookDetail,
  AudiobookIndexPageProps,
  SyncBatch,
} from "./types";

const boundaries = vi.hoisted(() => ({
  player: vi.fn(() => {
    throw new Error("Grouping lifecycle must not open playback");
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
    throw new Error("Grouping lifecycle must not create audio");
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

test("account replacement and unmount abort held confirmation bodies and reject their late navigation", async () => {
  const alice = audiobookGrouping();
  const bobPlaylist = {
    id: "93cd4519-2eb9-48a8-9157-2be45d1e7d2a",
    name: "Bob private chapter selection",
    kind: "local",
  };
  const bob: AudiobookIndexPageProps = {
    ...alice.index,
    user: { id: "bob", username: "Bob", admin: false },
    books: [],
    playlists: [bobPlaylist],
    grouping_playlists: [bobPlaylist],
  };
  const bobDetail: AudiobookDetail = {
    ...alice.confirmed,
    playlist: bobPlaylist,
  };
  const heldAlice = heldAudiobookBody(alice.confirmed);
  const heldBob = heldAudiobookBody(bobDetail);
  const requests: { account: string; method: string; signal: AbortSignal }[] =
    [];
  request.mockImplementation(async (input, options) => {
    const account = new Headers(options?.headers).get("x-hito-account");
    if (account !== alice.index.user.id && account !== bob.user.id)
      throw new Error("Unexpected grouping account");
    if (!(options?.signal instanceof AbortSignal))
      throw new Error("Grouping requires its account-owned signal");
    expect(options.credentials).toBe("same-origin");
    expect(new Headers(options.headers).get("x-hito-request")).toBe("1");
    const playlist = account === bob.user.id ? bobPlaylist : alice.playlist;
    requests.push({
      account,
      method: options.method ?? "GET",
      signal: options.signal,
    });
    if (input === "/api/v1/sync" && options.method === "POST") {
      const batch = JSON.parse(String(options.body)) as SyncBatch;
      expect(batch.operations.map((operation) => operation.action)).toEqual([
        { kind: "audiobook_create", playlist_id: playlist.id },
      ]);
      return Response.json({
        acknowledged: batch.operations.map((operation) => operation.id),
      });
    }
    if (
      input === `/api/v1/audiobooks/${playlist.id}` &&
      options.method === "GET"
    )
      return account === bob.user.id ? heldBob.response : heldAlice.response;
    throw new Error("Unexpected grouping lifecycle request");
  });
  let view: ReturnType<typeof render> | undefined;
  try {
    view = render(createElement(AudiobookIndexPage, alice.index));
    expect(request).not.toHaveBeenCalled();
    fireEvent.change(screen.getByRole("combobox", { name: "Playlist" }), {
      target: { value: alice.playlist.id },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add audiobook" }));
    await waitFor(() => expect(heldAlice.response.bodyUsed).toBe(true));
    expect(requests).toHaveLength(2);
    expect(requests[1].signal.aborted).toBe(false);

    view.rerender(createElement(AudiobookIndexPage, bob));
    expect(requests[1].signal.aborted).toBe(true);
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
    expect(request).toHaveBeenCalledTimes(2);
    await act(async () => heldAlice.release());
    expect(boundaries.visit).not.toHaveBeenCalled();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByText(alice.playlist.name, { exact: true })).toBeNull();

    fireEvent.change(screen.getByRole("combobox", { name: "Playlist" }), {
      target: { value: bobPlaylist.id },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add audiobook" }));
    await waitFor(() => expect(heldBob.response.bodyUsed).toBe(true));
    expect(requests).toHaveLength(4);
    expect(requests[3].signal.aborted).toBe(false);
    view.unmount();
    expect(requests[3].signal.aborted).toBe(true);
    await act(async () => heldBob.release());
    expect(boundaries.visit).not.toHaveBeenCalled();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(document.body.textContent?.includes(bobPlaylist.name)).toBe(false);
    expect(requests.map(({ account, method }) => [account, method])).toEqual([
      [alice.index.user.id, "POST"],
      [alice.index.user.id, "GET"],
      [bob.user.id, "POST"],
      [bob.user.id, "GET"],
    ]);
    expect(requests.every(({ signal }) => signal.aborted)).toBe(true);
  } finally {
    view?.unmount();
    await act(async () => {
      await heldAlice.dispose();
      await heldBob.dispose();
    });
  }
});
