import type { Schema } from "../../../shared/schema";
import { createElement, type AnchorHTMLAttributes } from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import CatalogPage from "../pages/CatalogPage";

const player = vi.hoisted(() => ({
  playQueue: vi.fn(),
  addToQueue: vi.fn().mockResolvedValue(undefined),
  subscribe: () => () => {},
  snapshot: () => ({ current: null }),
}));
vi.mock("../../playback/components/PlaybackProvider", () => ({
  usePlayer: () => player,
}));
vi.mock("../../../shared/hooks/useMutation", () => ({
  useMutation: () => ({
    mutate: vi.fn(),
    mutateMany: vi.fn(),
    pending: false,
  }),
}));
vi.mock("../../playback/components/TrackRowActions", () => ({
  default: () => null,
}));
vi.mock("../../artwork/components/Artwork", () => ({ default: () => null }));
vi.mock("../../library/hooks/useScanJob", () => ({
  useScanJob: () => ({ scan: vi.fn(), scanning: false }),
}));
vi.mock("@inertiajs/react", () => ({
  router: { get: vi.fn() },
  Link: ({ children, ...props }: AnchorHTMLAttributes<HTMLAnchorElement>) =>
    createElement("a", props, children),
}));

afterEach(() => {
  cleanup();
  player.addToQueue.mockClear();
});

function track(index: number, available = true): Schema<"Recording"> {
  return {
    id: `r${index}`,
    library_id: "personal",
    release_id: "album",
    title: `Track ${index}`,
    inherited_title: `Track ${index}`,
    artist: "Performer",
    inherited_artist: "Performer",
    album: "Album",
    duration_ms: 1000,
    available,
    plays_from_services: false,
    liked: false,
  } as Schema<"Recording">;
}

function open() {
  render(
    createElement(CatalogPage, {
      errors: {},
      native_playback: false,
      lock: { pin_enabled: false, idle_minutes: 0, locked: false },
      user: { id: "account", username: "Listener", admin: true },
      libraries: [{ id: "personal", name: "Personal", personal: true }],
      playlists: [],
      query: "",
      files: 3,
      recordings: {
        items: [track(1), track(2), track(3, false)],
        total: 3,
        offset: 0,
        limit: 100,
      },
    } as Schema<"CatalogPageProps">),
  );
}

test("tracks show where they play from and offer Find when no source exists", () => {
  open();
  expect(screen.getAllByText("Ready · Local")).toHaveLength(2);
  expect(screen.getByText("No source")).toBeTruthy();
  expect(screen.getByRole("link", { name: "Find" })).toBeTruthy();
});

test("choosing rows shows a count, can select every loaded row and clears again", () => {
  open();
  expect(
    screen.queryByRole("toolbar", { name: "Selection actions" }),
  ).toBeNull();
  fireEvent.click(screen.getByRole("checkbox", { name: "Select Track 1" }));
  const bar = screen.getByRole("toolbar", { name: "Selection actions" });
  expect(bar.textContent).toContain("1 selected");
  fireEvent.click(screen.getByRole("button", { name: "Select the 3 loaded" }));
  expect(bar.textContent).toContain("3 selected");
  expect(
    screen.queryByRole("button", { name: /Select the 3 loaded/ }),
  ).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Clear selection" }));
  expect(
    screen.queryByRole("toolbar", { name: "Selection actions" }),
  ).toBeNull();
});

test("Add to queue adds the chosen rows in order and clears the choice", async () => {
  open();
  fireEvent.click(screen.getByRole("checkbox", { name: "Select Track 2" }));
  fireEvent.click(screen.getByRole("checkbox", { name: "Select Track 1" }));
  fireEvent.click(screen.getByRole("button", { name: "Add to queue" }));
  await vi.waitFor(() => expect(player.addToQueue).toHaveBeenCalledTimes(2));
  expect(player.addToQueue.mock.calls.map(([added]) => added.id)).toEqual([
    "r1",
    "r2",
  ]);
  await vi.waitFor(() =>
    expect(
      screen.queryByRole("toolbar", { name: "Selection actions" }),
    ).toBeNull(),
  );
});

test("each library row keeps its own play button beside the choice box", () => {
  open();
  fireEvent.click(screen.getByRole("button", { name: "Play Track 2" }));
  expect(player.playQueue).toHaveBeenCalledTimes(1);
  expect(player.playQueue.mock.calls[0][1]).toBe(1);
});
