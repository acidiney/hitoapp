import type { Schema } from "../../../shared/schema";
import { createElement, type AnchorHTMLAttributes } from "react";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import TrackTable, {
  type CatalogControls,
  type CatalogRowView,
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

const controls: CatalogControls = {
  play: vi.fn(),
  find: vi.fn(),
  open: vi.fn(),
  like: vi.fn(),
  mutated: vi.fn(),
};

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

function catalogRow(
  id: string,
  title: string,
  over: Partial<CatalogRowView> = {},
): TrackTableRow {
  return {
    kind: "catalog",
    row: {
      id,
      number: "1",
      title,
      artist: "Ana Moura",
      album: null,
      artistShown: false,
      durationMs: 1000,
      status: "idle",
      recording: null,
      ...over,
    },
  };
}

function show(rows: TrackTableRow[]) {
  render(
    createElement(TrackTable, {
      rows,
      catalog: controls,
      playlists: [] as Schema<"Playlist">[],
      mutate: vi.fn(async () => ({ ok: true as const })),
    }),
  );
  return screen.getAllByRole("row").slice(1);
}

const found = (id: string, title: string) =>
  catalogRow(`occ-${id}`, title, {
    status: "found",
    recording: recording(id, title),
  });

test("the playing catalog row shows bars, is announced and is the only current row", () => {
  store.state = { current: { id: "b" }, playing: true };
  const rows = show([
    found("a", "First"),
    found("b", "Second"),
    catalogRow("occ-c", "Third"),
  ]);
  const current = rows.filter(
    (row) => row.getAttribute("aria-current") === "true",
  );
  expect(current).toHaveLength(1);
  expect(current[0].textContent).toContain("Second");
  expect(current[0].className).toContain("playing-row");
  expect(current[0].querySelectorAll(".playing-bars")).toHaveLength(1);
  expect(current[0].textContent).toContain("Now playing");
  expect(rows[0].querySelector(".playing-bars")).toBeNull();
});

test("a busy catalog row has one subtitle line with the status and no repeated album", () => {
  const [row] = show([
    catalogRow("occ-a", "Até ao verão", { status: "finding" }),
  ]);
  const lines = row.querySelectorAll(".track-sub");
  expect(lines).toHaveLength(1);
  expect(within(lines[0] as HTMLElement).getByRole("status").textContent).toBe(
    "Checking…",
  );
  expect(lines[0].textContent).not.toContain("Ana Moura");
  expect(row.textContent).not.toContain("Best of");
});

test("a catalog row shows the artist only when it differs from the album artist", () => {
  const rows = show([
    catalogRow("occ-a", "Same"),
    catalogRow("occ-b", "Guest", { artist: "Guest Singer", artistShown: true }),
  ]);
  expect(rows[0].querySelector(".track-meta")).toBeNull();
  expect(rows[0].textContent).toContain("Not in your library");
  expect(rows[1].querySelector(".track-meta")?.textContent).toBe(
    "Guest Singer",
  );
});

test("an unfound catalog row has one trailing control besides the menu", () => {
  const [row] = show([catalogRow("occ-a", "Valentim")]);
  const buttons = within(row).getAllByRole("button");
  expect(buttons.map((button) => button.getAttribute("aria-label"))).toEqual([
    "Play Valentim",
    "Find audio for Valentim",
    "Actions for Valentim",
  ]);
  expect(within(row).queryByLabelText(/Find audio and like/)).toBeNull();
});

function onPhone(matches: boolean) {
  window.matchMedia = ((query: string) => ({
    matches,
    media: query,
    addEventListener: () => {},
    removeEventListener: () => {},
  })) as unknown as typeof window.matchMedia;
}

test("on phone a click in the row plays it and a click on the menu does not", () => {
  onPhone(true);
  const play = vi.fn();
  const [row] = (() => {
    render(
      createElement(TrackTable, {
        rows: [catalogRow("occ-a", "Valentim")],
        catalog: { ...controls, play },
        playlists: [] as Schema<"Playlist">[],
        mutate: vi.fn(async () => ({ ok: true as const })),
      }),
    );
    return screen.getAllByRole("row").slice(1);
  })();
  fireEvent.click(
    within(row).getByRole("button", { name: "Actions for Valentim" }),
  );
  expect(play).not.toHaveBeenCalled();
  fireEvent.click(row.querySelector("td.track-cell")!);
  expect(play).toHaveBeenCalledWith(0);
});

test("off phone a click in the row does not play it", () => {
  onPhone(false);
  const play = vi.fn();
  render(
    createElement(TrackTable, {
      rows: [catalogRow("occ-a", "Valentim")],
      catalog: { ...controls, play },
      playlists: [] as Schema<"Playlist">[],
      mutate: vi.fn(async () => ({ ok: true as const })),
    }),
  );
  fireEvent.click(
    screen.getAllByRole("row")[1].querySelector("td.track-cell")!,
  );
  expect(play).not.toHaveBeenCalled();
});
