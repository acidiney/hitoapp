import { entryText } from "../../../test/rows";
import type { Schema } from "../../../shared/schema";
import { createElement, type ReactNode } from "react";
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import PlaylistPage from "../pages/PlaylistPage";
import { ArtworkProvider } from "../../artwork/components/ArtworkProvider";
import { NoticeProvider } from "../../../shared/ui/Notices";
import {
  PlaybackProvider,
  usePlayer,
} from "../../playback/components/PlaybackProvider";
import type { Player } from "../../playback/model";

import { copiedPlaylistPage, recordingA } from "../fixtures/playlistPages";

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ children, href }: { children: ReactNode; href: string }) =>
    createElement("a", { href }, children),
}));

test("an imported playlist retrieves its own cover, retries failures and cancels late account work while playback stays usable", async () => {
  vi.useFakeTimers();
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const warn = vi.spyOn(console, "warn");
  let props: Schema<"PlaylistPageProps"> = structuredClone(copiedPlaylistPage);
  const playlist = props.detail.playlist.id;
  const replacement = "93bc0ccf-84ea-4918-8081-ac744e847d96";
  const image = `/api/v1/playlists/${playlist}/artwork`;
  const availability = `${image}/availability`;
  const enqueue = `${image}/fetch`;
  const first: Schema<"PlaylistArtworkFetch"> = {
    id: "first-playlist-cover",
    playlist_id: playlist,
    status: "queued",
    attempts: 0,
    error: null,
    created_at: 1,
    updated_at: 1,
  };
  const retry = {
    ...first,
    id: "retry-playlist-cover",
    created_at: 2,
    updated_at: 2,
  };
  const ready: Schema<"PlaylistArtworkFetch"> = {
    ...retry,
    status: "ready",
    attempts: 1,
    updated_at: 3,
  };
  const firstStatus = `/api/v1/playlist-artwork-fetches/${first.id}`;
  const retryStatus = `/api/v1/playlist-artwork-fetches/${retry.id}`;
  let body!: ReadableStreamDefaultController<Uint8Array>;
  const heldBody = new Response(
    new ReadableStream<Uint8Array>({
      start(controller) {
        body = controller;
      },
    }),
    {
      headers: { "content-type": "application/json" },
    },
  );
  let enqueues = 0;
  const accountFor = (options?: RequestInit) =>
    new Headers(options?.headers).get("x-hito-account");
  const fetch = vi.fn((path: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    const account = accountFor(options);
    if (path.endsWith("/download") && method === "GET")
      return Promise.resolve(
        Response.json({ error: "Not found" }, { status: 404 }),
      );
    if (
      path === `/api/v1/recordings/${recordingA}/playback-sources` &&
      method === "GET"
    )
      return Promise.resolve(
        Response.json([
          {
            id: "owned-audio",
            recording_id: recordingA,
            provider: "fixture",
            device_id: "server",
            ready: true,
          },
        ]),
      );
    if (path === availability && method === "GET")
      return Promise.resolve(Response.json({ can_fetch: account === "alice" }));
    if (path === enqueue && method === "POST" && account === "alice") {
      enqueues++;
      return Promise.resolve(
        Response.json(enqueues === 1 ? first : enqueues === 2 ? retry : ready, {
          status: 202,
        }),
      );
    }
    if (path === firstStatus && method === "GET" && account === "alice")
      return Promise.resolve(
        Response.json({
          ...first,
          status: "failed",
          attempts: 3,
          error: "not_available",
          updated_at: 2,
        }),
      );
    if (path === retryStatus && method === "GET" && account === "alice")
      return Promise.resolve(heldBody);
    if (
      path === `/api/v1/playlists/${replacement}/artwork/availability` &&
      method === "GET" &&
      account === "alice"
    )
      return Promise.resolve(Response.json({ can_fetch: true }));
    if (
      path === `/api/v1/playlists/${replacement}/artwork/fetch` &&
      method === "POST" &&
      account === "alice"
    )
      return Promise.resolve(
        Response.json(
          { ...ready, id: "replacement-cover", playlist_id: replacement },
          { status: 202 },
        ),
      );
    throw new Error(
      "Unexpected request outside playlist artwork or existing playback reads",
    );
  });
  vi.stubGlobal("fetch", fetch);
  let player!: Player;
  function Page() {
    player = usePlayer();
    return createElement(PlaylistPage, props);
  }
  const view = () =>
    createElement(NoticeProvider, {
      key: props.user.id,
      children: createElement(ArtworkProvider, {
        account: props.user.id,
        children: createElement(PlaybackProvider, {
          account: props.user.id,
          children: createElement(Page),
        }),
      }),
    });
  const { container, rerender, unmount } = render(view());
  await act(async () => {});
  const table = screen.getByRole("table", { name: "Playlist entries" });
  const entries = within(table)
    .getAllByRole("row")
    .slice(1)
    .map(entryText);
  await act(async () => {
    fireEvent.click(
      within(table).getAllByRole("button", { name: "Play Fixture A" })[0],
    );
  });
  expect(player.snapshot().current?.id).toBe(recordingA);
  expect(play).toHaveBeenCalledTimes(1);
  const originalPlayer = player;
  const originalSelection = player.queueSnapshot();
  const originalQueue = await player.queuePage(0, 10);
  const hero = () => container.querySelector(".playlist-header .artwork img");
  expect(hero()?.getAttribute("src")).toBe(image);
  const artworkCalls = () =>
    fetch.mock.calls.filter(([path]) => path.includes("artwork"));
  expect(artworkCalls()).toEqual([]);

  await act(async () => {
    fireEvent.error(hero()!);
  });
  expect(artworkCalls().map(([path]) => path)).toEqual([availability, enqueue]);
  expect(hero()).toBeNull();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(1000);
  });
  const retryButton = screen.getByRole("button", { name: "Retry artwork" });
  expect((retryButton as HTMLButtonElement).disabled).toBe(false);
  expect(screen.getByText("Artwork could not be retrieved.")).toBeTruthy();
  expect(warn).toHaveBeenCalledWith(
    "[hito]",
    expect.objectContaining({
      event: "artwork.finished",
      playlist_id: playlist,
      job_id: first.id,
      reason: "failed",
    }),
  );
  await act(async () => {
    fireEvent.click(retryButton);
  });
  expect(enqueues).toBe(2);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(1000);
  });
  const held = fetch.mock.calls.find(([path]) => path === retryStatus)![1];
  expect(held?.signal?.aborted).toBe(false);
  expect(player).toBe(originalPlayer);
  expect(player.queueSnapshot()).toEqual(originalSelection);
  expect(await player.queuePage(0, 10)).toEqual(originalQueue);
  expect(player.snapshot().current?.id).toBe(recordingA);
  expect(play).toHaveBeenCalledTimes(1);
  expect(
    within(table)
      .getAllByRole("row")
      .slice(1)
      .map(entryText),
  ).toEqual(entries);

  props = { ...props, user: { id: "bob", username: "Bob", admin: false } };
  rerender(view());
  expect(held?.signal?.aborted).toBe(true);
  expect(hero()?.getAttribute("src")).toBe(image);
  await act(async () => {
    fireEvent.error(hero()!);
  });
  expect(hero()).toBeNull();
  expect(
    artworkCalls()
      .filter(([, options]) => accountFor(options) === "bob")
      .map(([path]) => path),
  ).toEqual([availability]);
  await act(async () => {
    body.enqueue(new TextEncoder().encode(JSON.stringify(ready)));
    body.close();
  });
  expect(hero()).toBeNull();
  expect(screen.queryByText("Artwork could not be retrieved.")).toBeNull();

  props = { ...props, user: copiedPlaylistPage.user };
  rerender(view());
  await act(async () => {
    fireEvent.error(hero()!);
  });
  expect(enqueues).toBe(3);
  expect(hero()?.getAttribute("src")).toBe(image);
  expect(screen.queryByRole("button", { name: "Retry artwork" })).toBeNull();
  props = {
    ...props,
    detail: {
      ...props.detail,
      playlist: { ...props.detail.playlist, id: replacement },
    },
  };
  rerender(view());
  expect(hero()?.getAttribute("src")).toBe(
    `/api/v1/playlists/${replacement}/artwork`,
  );
  await act(async () => {
    fireEvent.error(hero()!);
  });
  expect(hero()?.getAttribute("src")).toBe(
    `/api/v1/playlists/${replacement}/artwork`,
  );
  expect(artworkCalls().map(([path]) => path)).toEqual([
    availability,
    enqueue,
    firstStatus,
    availability,
    enqueue,
    retryStatus,
    availability,
    availability,
    enqueue,
    `/api/v1/playlists/${replacement}/artwork/availability`,
    `/api/v1/playlists/${replacement}/artwork/fetch`,
  ]);
  for (const [path, options] of artworkCalls()) {
    expect(path).not.toContain("/recordings/");
    expect(path).not.toContain("/releases/");
    expect(options?.credentials).toBe("same-origin");
    expect(options?.headers).toEqual(
      expect.objectContaining({ "x-hito-request": "1" }),
    );
    expect(options?.signal).toBeInstanceOf(AbortSignal);
    expect(options?.body).toBeUndefined();
  }
  unmount();
  await act(async () => {});
  expect(vi.getTimerCount()).toBe(0);
});
