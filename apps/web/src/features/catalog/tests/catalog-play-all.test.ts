import type { Schema } from "../../../shared/schema";
import { createElement, type AnchorHTMLAttributes } from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import CatalogResults from "../components/CatalogResults";
import AlbumPage from "../pages/AlbumPage";
import { Playback } from "../../playback/playback";
import {
  QueueMedia,
  queueTrack,
} from "../../playback/fixtures/browserQueueMedia";
import { deferred } from "../../../test/async";

let player: Playback;
vi.mock("../../playback/components/PlaybackProvider", () => ({
  usePlayer: () => player,
}));
vi.mock("../../../shared/hooks/useMutation", () => ({
  useMutation: () => ({ mutate: vi.fn(), pending: false }),
}));
vi.mock("../../playback/components/TrackActions", () => ({
  default: () => null,
}));
vi.mock("../../artwork/components/Artwork", () => ({ default: () => null }));
vi.mock("../../artwork/components/ArtworkStatus", () => ({
  default: () => null,
}));
vi.mock("@inertiajs/react", () => ({
  router: { get: vi.fn() },
  Link: ({ children, ...props }: AnchorHTMLAttributes<HTMLAnchorElement>) =>
    createElement("a", props, children),
}));

const recording = (index: number) => ({
  ...queueTrack(`r${index}`, `Track ${index}`),
  library_id: "personal",
  inherited_title: `Track ${index}`,
  inherited_artist: "Owned performer",
  liked: false,
});
const items = (from: number, count: number) =>
  Array.from({ length: count }, (_, i) => recording(from + i));
const user = { id: "account", username: "Listener", admin: false };
const requested: string[] = [];
let respond: (url: string, signal: AbortSignal) => Promise<Response>;

beforeEach(() => {
  requested.length = 0;
  const media = new QueueMedia();
  player = new Playback(media, async (id) => [{ id, ready: true }]);
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, options: RequestInit) => {
      requested.push(url);
      return respond(url, options.signal!);
    }),
  );
});
afterEach(() => {
  cleanup();
  window.history.replaceState(null, "", "/");
  vi.unstubAllGlobals();
});

function library(total: number, offset: number, query = "", path = "/library") {
  const page = {
    total,
    offset,
    limit: 200,
    items: items(offset, Math.min(200, total - offset)),
  };
  return createElement(CatalogResults, {
    user,
    playlists: [],
    query,
    recordings: page,
    path,
    emptyTitle: "Empty",
    emptyText: "Empty",
  });
}
const pageOf = (url: string, total: number) => {
  const params = new URL(url, "http://hito.test").searchParams;
  const offset = Number(params.get("offset"));
  const limit = Number(params.get("limit"));
  return Response.json({
    total,
    offset,
    limit,
    items: items(offset, Math.max(0, Math.min(limit, total - offset))),
  });
};

test("Play on a paged library queues every page, fetched one after another with the search kept", async () => {
  window.history.replaceState(null, "", "/library?q=blue&library=main");
  respond = async (url) => pageOf(url, 450);
  render(library(450, 0, "blue"));
  fireEvent.click(screen.getByRole("button", { name: "Play tracks" }));
  await vi.waitFor(() => expect(player.queueSnapshot().total).toBe(450));
  expect(requested).toEqual([
    "/api/v1/recordings?q=blue&library=main&offset=200&limit=200",
    "/api/v1/recordings?q=blue&library=main&offset=400&limit=200",
  ]);
  expect(player.queueSnapshot().pending).toBe(0);
  expect((await player.queuePage(449, 1)).entries[0].recording.title).toBe(
    "Track 449",
  );
});

test("liked tracks ask for liked recordings only", async () => {
  respond = async (url) => pageOf(url, 250);
  render(library(250, 0, "", "/likes"));
  fireEvent.click(screen.getByRole("button", { name: "Play tracks" }));
  await vi.waitFor(() => expect(player.queueSnapshot().total).toBe(250));
  expect(requested).toEqual([
    "/api/v1/recordings?liked=true&offset=200&limit=200",
  ]);
});

test("Play from the second page starts at the beginning of the whole result", async () => {
  respond = async (url) => pageOf(url, 450);
  render(library(450, 200));
  fireEvent.click(screen.getByRole("button", { name: "Play tracks" }));
  await vi.waitFor(() => expect(player.queueSnapshot().total).toBe(450));
  expect(requested[0]).toBe("/api/v1/recordings?offset=0&limit=200");
  expect((await player.queuePage(0, 1)).entries[0].recording.title).toBe(
    "Track 0",
  );
  expect(player.snapshot().current?.title).toBe("Track 0");
});

test("a row click plays from that row and keeps loading the later pages", async () => {
  respond = async (url) => pageOf(url, 450);
  render(library(450, 200));
  fireEvent.click(screen.getByRole("button", { name: "Play Track 205" }));
  await vi.waitFor(() => expect(player.queueSnapshot().total).toBe(250));
  expect(requested).toEqual(["/api/v1/recordings?offset=400&limit=200"]);
  expect(player.snapshot().current?.title).toBe("Track 205");
});

test("a complete page does not fetch anything", () => {
  respond = async () => {
    throw new Error("no request expected");
  };
  render(library(120, 0));
  fireEvent.click(screen.getByRole("button", { name: "Play tracks" }));
  expect(requested).toEqual([]);
  expect(player.queueSnapshot().total).toBe(120);
});

test("starting something else or switching account cancels the page in flight", async () => {
  const held = deferred<Response>();
  const signals: AbortSignal[] = [];
  respond = async (_url, signal) => {
    signals.push(signal);
    return held.promise;
  };
  render(library(450, 0));
  fireEvent.click(screen.getByRole("button", { name: "Play tracks" }));
  await vi.waitFor(() => expect(signals).toHaveLength(1));
  await player.playQueue([queueTrack("other")]);
  expect(signals[0].aborted).toBe(true);
  held.resolve(pageOf("/x?offset=200&limit=200", 450));
  await new Promise((resolve) => setTimeout(resolve, 20));
  expect(player.queueSnapshot().total).toBe(1);

  const second = deferred<Response>();
  respond = async (_url, signal) => {
    signals.push(signal);
    return second.promise;
  };
  fireEvent.click(screen.getByRole("button", { name: "Play tracks" }));
  await vi.waitFor(() => expect(signals).toHaveLength(2));
  player.retire();
  expect(signals[1].aborted).toBe(true);
});

test("a failed page leaves the loaded tracks playing and says the rest is missing", async () => {
  respond = async () => new Response("{}", { status: 500 });
  render(library(450, 0));
  fireEvent.click(screen.getByRole("button", { name: "Play tracks" }));
  await vi.waitFor(() =>
    expect(player.snapshot().error).toMatch(/rest of the list/i),
  );
  expect(player.queueSnapshot().total).toBe(200);
  expect(player.snapshot().playing).toBe(true);
});

test("a paged album queues all of its tracks in tag order", async () => {
  const track = (index: number) => ({
    ...recording(index),
    entry_id: `entry-${index}`,
    release_id: "album",
    disc_number: 1,
    track_number: index + 1,
  });
  const all = Array.from({ length: 450 }, (_, i) => track(i));
  respond = async (url) => {
    const params = new URL(url, "http://hito.test").searchParams;
    const offset = Number(params.get("offset"));
    return Response.json({
      release: props.detail.release,
      catalog_anchor: null,
      saved_edition: null,
      tracks: {
        total: 450,
        offset,
        limit: 200,
        items: all.slice(offset, offset + 200),
      },
    });
  };
  const props = {
    errors: {},
    native_playback: false,
    lock: { pin_enabled: false, idle_minutes: 0, locked: false },
    user,
    libraries: [],
    playlists: [],
    detail: {
      release: {
        id: "album",
        library_id: "personal",
        title: "Long album",
        artist: "Performer",
        artist_id: "artist",
        track_count: 450,
        duration_ms: 450000,
        artwork_recording_id: "r0",
      },
      tracks: { total: 450, offset: 0, limit: 200, items: all.slice(0, 200) },
      catalog_anchor: null,
      saved_edition: null,
    },
  } as unknown as Schema<"AlbumPageProps">;
  render(createElement(AlbumPage, props));
  fireEvent.click(screen.getByRole("button", { name: "Play album" }));
  await vi.waitFor(() => expect(player.queueSnapshot().total).toBe(450));
  expect(requested).toEqual([
    "/api/v1/releases/album?offset=200&limit=200",
    "/api/v1/releases/album?offset=400&limit=200",
  ]);
});

test("leaving the page cancels the fetch for the first page of the result", async () => {
  const held = deferred<Response>();
  const signals: AbortSignal[] = [];
  respond = async (_url, signal) => {
    signals.push(signal);
    return held.promise;
  };
  const view = render(library(450, 200));
  fireEvent.click(screen.getByRole("button", { name: "Play tracks" }));
  await vi.waitFor(() => expect(signals).toHaveLength(1));
  view.unmount();
  expect(signals[0].aborted).toBe(true);
  held.resolve(pageOf("/x?offset=0&limit=200", 450));
  await new Promise((resolve) => setTimeout(resolve, 20));
  expect(player.queueSnapshot().total).toBe(0);
});

test("when the first page cannot be fetched the loaded page plays and the listener is told the queue is partial", async () => {
  respond = async (url) =>
    url.includes("offset=0")
      ? new Response("{}", { status: 500 })
      : pageOf(url, 450);
  render(library(450, 200));
  fireEvent.click(screen.getByRole("button", { name: "Play tracks" }));
  await vi.waitFor(() =>
    expect(player.snapshot().error).toMatch(/Only this page was queued/),
  );
  expect(player.queueSnapshot().total).toBe(200);
  expect(player.snapshot().current?.title).toBe("Track 200");
});
