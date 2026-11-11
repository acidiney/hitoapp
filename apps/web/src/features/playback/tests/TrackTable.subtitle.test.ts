import type { Schema } from "../../../shared/schema";
import { createElement, type AnchorHTMLAttributes } from "react";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import TrackTable, {
  type TrackRow,
  type TrackTableRow,
} from "../components/TrackTable";

const store = vi.hoisted(() => ({
  state: { current: null as null | { id: string }, playing: true },
}));
const player = vi.hoisted(() => ({
  playQueue: vi.fn(async () => {}),
  subscribe: () => () => {},
  snapshot: () => store.state,
}));
vi.mock("../components/PlaybackProvider", () => ({ usePlayer: () => player }));
vi.mock("../../artwork/components/Artwork", () => ({ default: () => null }));
vi.mock("@inertiajs/react", () => ({
  router: { visit: vi.fn(), reload: vi.fn() },
  Link: ({ children, ...props }: AnchorHTMLAttributes<HTMLAnchorElement>) =>
    createElement("a", props, children),
}));

afterEach(() => {
  cleanup();
  store.state = { current: null, playing: true };
});


function recording(id: string, title: string): TrackRow {
  return {
    id,
    row_id: id,
    title,
    artist: "Ana Moura",
    duration_ms: 1000,
    available: true,
    liked: false,
    inherited_title: title,
    inherited_artist: "Ana Moura",
    plays_from_services: false,
  } as TrackRow;
}

function show(rows: TrackTableRow[]) {
  render(
    createElement(TrackTable, {
      rows,
      playlists: [] as Schema<"Playlist">[],
      mutate: vi.fn(async () => ({ ok: true as const })),
    }),
  );
  return screen.getAllByRole("row").slice(1);
}

test("a no-source row keeps its status and subtitle in one line", () => {
  const [row] = show([
    {
      kind: "recording",
      track: {
        ...recording("n", "Desfado"),
        available: false,
        plays_from_services: false,
        album: "Desfado",
      },
    },
  ]);
  const line = row.querySelector(".track-sub")!;
  expect(line.querySelector("small")?.textContent).toBe("No source");
  expect(line.querySelector(".track-meta")?.textContent).toContain("Desfado");
});

test("a row without a subtitle has an empty subtitle line", () => {
  const [row] = show([
    {
      kind: "recording",
      track: { ...recording("p", "Caso Arrumado"), artist: "" },
    },
  ]);
  expect(row.querySelector(".track-sub")?.children).toHaveLength(0);
});
