import type { Schema } from "../../../shared/schema";
import { createElement, type AnchorHTMLAttributes } from "react";
import { act, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import TrackTable, {
  type TrackRow,
  type TrackTableRow,
} from "../components/TrackTable";

type Listener = () => void;
const store = vi.hoisted(() => ({
  state: {
    current: null as null | { id: string; entry_id?: string },
    playing: false,
  },
  listeners: new Set<() => void>(),
  positionListeners: new Set<() => void>(),
  renders: 0,
}));
const player = vi.hoisted(() => ({
  playQueue: vi.fn(async () => {}),
  subscribe: (listener: Listener) => {
    store.listeners.add(listener);
    return () => store.listeners.delete(listener);
  },
  subscribePosition: (listener: Listener) => {
    store.positionListeners.add(listener);
    return () => store.positionListeners.delete(listener);
  },
  snapshot: () => store.state,
}));
vi.mock("../components/PlaybackProvider", () => ({ usePlayer: () => player }));
vi.mock("../../artwork/components/Artwork", () => ({
  default: () => {
    store.renders += 1;
    return null;
  },
}));
vi.mock("@inertiajs/react", () => ({
  router: { visit: vi.fn(), reload: vi.fn() },
  Link: ({ children, ...props }: AnchorHTMLAttributes<HTMLAnchorElement>) =>
    createElement("a", props, children),
}));

function row(id: string, rowId: string, entry?: string): TrackTableRow {
  const track = {
    id,
    row_id: rowId,
    entry_id: entry,
    title: `Title ${rowId}`,
    artist: "Artist",
    duration_ms: 1000,
    available: true,
    liked: false,
    inherited_title: `Title ${rowId}`,
    inherited_artist: "Artist",
    plays_from_services: false,
  } as TrackRow;
  return { kind: "recording", track };
}

function show(rows: TrackTableRow[]) {
  render(
    createElement(TrackTable, {
      rows,
      playlists: [] as Schema<"Playlist">[],
      mutate: vi.fn(async () => ({ ok: true as const })),
    }),
  );
}

function change(state: typeof store.state) {
  act(() => {
    store.state = state;
    store.listeners.forEach((listener) => listener());
  });
}

const playingRows = () =>
  screen
    .getAllByRole("row")
    .filter((item) => item.getAttribute("aria-current") === "true");

test("the playing row shows bars and is announced, the others do not", () => {
  store.state = { current: { id: "b" }, playing: true };
  show([row("a", "1"), row("b", "2")]);
  const [active] = playingRows();
  expect(playingRows()).toHaveLength(1);
  expect(active.className).toContain("playing-row");
  expect(active.querySelectorAll(".playing-bars")).toHaveLength(1);
  expect(
    active.querySelector(".playing-bars")?.getAttribute("data-state"),
  ).toBe("playing");
  expect(active.textContent).toContain("Now playing");
});

test("pausing holds the bars still and says Paused", () => {
  store.state = { current: { id: "a" }, playing: true };
  show([row("a", "1")]);
  change({ current: { id: "a" }, playing: false });
  const [active] = playingRows();
  expect(
    active.querySelector(".playing-bars")?.getAttribute("data-state"),
  ).toBe("paused");
  expect(active.textContent).toContain("Paused");
  expect(active.textContent).not.toContain("Now playing");
});

test("a recording repeated in a playlist highlights the entry that plays", () => {
  store.state = { current: { id: "a", entry_id: "e2" }, playing: true };
  show([row("a", "1", "e1"), row("b", "2", "e9"), row("a", "3", "e2")]);
  const rows = screen.getAllByRole("row").slice(1);
  expect(rows.map((item) => item.className.includes("playing-row"))).toEqual([
    false,
    false,
    true,
  ]);
});

test("without an entry id every row of the recording is highlighted", () => {
  store.state = { current: { id: "a" }, playing: true };
  show([row("a", "1", "e1"), row("a", "2", "e2"), row("b", "3", "e3")]);
  expect(playingRows()).toHaveLength(2);
});

test("position ticks do not render the rows again", () => {
  store.state = { current: { id: "a" }, playing: true };
  show([row("a", "1"), row("b", "2")]);
  const before = store.renders;
  act(() => {
    store.positionListeners.forEach((listener) => listener());
  });
  expect(store.renders).toBe(before);
  change({ current: { id: "b" }, playing: true });
  expect(store.renders).toBeGreaterThan(before);
});
