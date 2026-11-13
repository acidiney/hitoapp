import { createElement, type AnchorHTMLAttributes } from "react";
import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import TrackTable, { type TrackTableRow } from "../components/TrackTable";
import { TrackActionsProvider } from "../components/TrackActionsProvider";
import { sourceStatusStore } from "../../sources/model/sourceStatusStore";
import { NoticeProvider } from "../../../shared/ui/Notices";
import { invalidate } from "../../../shared/invalidation";

const player = vi.hoisted(() => ({
  playQueue: vi.fn(async () => {}),
  subscribe: () => () => {},
  snapshot: () => ({ current: null }),
}));
vi.mock("../components/PlaybackProvider", () => ({ usePlayer: () => player }));
vi.mock("../../artwork/components/Artwork", () => ({ default: () => null }));
vi.mock("@inertiajs/react", () => ({
  router: { visit: vi.fn(), reload: vi.fn() },
  Link: ({ children, ...props }: AnchorHTMLAttributes<HTMLAnchorElement>) =>
    createElement("a", props, children),
}));

let sources: unknown[] = [];
const fetch = vi.fn(async () => Response.json(sources));
const row = (id: string): TrackTableRow => ({
  kind: "recording",
  track: {
    id,
    row_id: `row-${id}`,
    title: `Title ${id}`,
    artist: "Artist",
    album: "Album",
    duration_ms: 1000,
    available: false,
    liked: false,
    plays_from_services: false,
  } as never,
});

beforeEach(() => {
  sourceStatusStore.reset();
  sources = [];
  vi.stubGlobal("fetch", fetch);
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

test("a playlist row flips from No source to Ready when its source is resolved, without a reload", async () => {
  const mutate = vi.fn(async () => ({ ok: true as const }));
  render(
    createElement(
      NoticeProvider,
      null,
      createElement(
        TrackActionsProvider,
        { account: "alice", playlists: [], children: null },
        createElement(TrackTable, {
          rows: [row("r1"), row("r2")],
          playlists: [],
          mutate,
          layout: "playlist",
        }),
      ),
    ),
  );
  expect(screen.getAllByText("No source")).toHaveLength(2);
  sources = [
    {
      id: "s1",
      recording_id: "r1",
      provider: "youtube",
      provider_label: "YouTube Music",
      label: "Opus",
      match_kind: "verified",
      ready: true,
      can_prepare: true,
    },
  ];
  await act(async () => {
    invalidate("resolved", "r1");
  });
  await waitFor(() =>
    expect(screen.getByText("Ready · from YouTube Music")).toBeTruthy(),
  );
  expect(screen.getAllByText("No source")).toHaveLength(1);
  expect(screen.getAllByRole("link", { name: "Find" })).toHaveLength(1);
  expect(fetch).toHaveBeenCalledTimes(1);
});
