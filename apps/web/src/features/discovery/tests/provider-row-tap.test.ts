import type { Schema } from "../../../shared/schema";
import { createElement, type ReactNode } from "react";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import DiscoverPage from "../pages/DiscoverPage";
import { skippingFeatured } from "../fixtures/skippingFeatured";

const { player } = vi.hoisted(() => ({
  player: {
    prepareSelection: vi.fn(() => vi.fn().mockResolvedValue(true)),
    addToQueue: vi.fn().mockResolvedValue(undefined),
    playNext: vi.fn().mockResolvedValue(undefined),
  },
}));
vi.mock("../../playback/components/PlaybackProvider", () => ({
  usePlayer: () => player,
}));
vi.mock("@inertiajs/react", () => ({
  Link: ({ href, children, ...rest }: { href: string; children: ReactNode }) =>
    createElement("a", { href, ...rest }, children),
}));
const connection: Schema<"PluginInstallation"> = {
  id: "connection",
  service_id: "service",
  revision: 1,
  checked_at: 1,
  credential_set: false,
  manifest: {
    id: "org.hito.fixture",
    name: "Fixture service",
    protocol_version: 1,
    capabilities: ["search", "sources"],
  },
};
const props: Schema<"DiscoverPageProps"> = {
  errors: {},
  native_playback: false,
  lock: { pin_enabled: false, idle_minutes: 0, locked: false },
  user: { id: "alice", username: "Alice", admin: false },
  libraries: [],
  playlists: [],
  connections: [connection],
  service_names: { service: "Home collection" },
  genres: [],
  artists: { items: [], total: 0, offset: 0, limit: 6 },
};

const hit = { id: "remote", title: "Provider song", artist: "Fixture" };
const adopted = {
  id: "saved",
  library_id: "lib",
  title: "Provider song",
  artist: "Fixture",
  inherited_title: "Provider song",
  inherited_artist: "Fixture",
  duration_ms: 1000,
  available: true,
  plays_from_services: true,
  liked: false,
};

async function searchOne(adoption: object) {
  const fetch = vi
    .fn()
    .mockResolvedValueOnce(new Response(JSON.stringify({ items: [hit] })))
    .mockImplementation(() =>
      Promise.resolve(new Response(JSON.stringify(adoption))),
    );
  vi.stubGlobal("fetch", skippingFeatured(fetch));
  render(createElement(DiscoverPage, props));
  fireEvent.change(screen.getByRole("searchbox", { name: "Search music" }), {
    target: { value: "song" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Search" }));
  return screen.findByRole("button", { name: "Play Provider song" });
}

test("the Play button is the row's tap target, shares the desktop play path and never contains the menu", async () => {
  player.prepareSelection.mockClear();
  const play = await searchOne(adopted);
  const row = play.closest("li") as HTMLElement;
  expect(play.querySelector("button, a")).toBeNull();
  expect(row.querySelectorAll("button[aria-label^='Play ']")).toHaveLength(1);
  const menu = screen.getByRole("button", {
    name: "Actions for Provider song",
  });
  expect(play.contains(menu)).toBe(false);
  fireEvent.click(play);
  await waitFor(() => expect(player.prepareSelection).toHaveBeenCalled());
});
