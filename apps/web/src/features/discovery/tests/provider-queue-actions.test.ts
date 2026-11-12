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
  await screen.findByRole("button", { name: "Play Provider song" });
}

test("a provider row queues the adopted recording through the same path as play", async () => {
  player.addToQueue.mockClear();
  player.playNext.mockClear();
  await searchOne(adopted);
  fireEvent.click(
    screen.getByRole("button", { name: "Actions for Provider song" }),
  );
  fireEvent.click(
    await screen.findByRole("menuitem", { name: "Add to queue" }),
  );
  await waitFor(() => expect(player.addToQueue).toHaveBeenCalledWith(adopted));
  fireEvent.click(
    screen.getByRole("button", { name: "Actions for Provider song" }),
  );
  fireEvent.click(await screen.findByRole("menuitem", { name: "Play next" }));
  await waitFor(() => expect(player.playNext).toHaveBeenCalledWith(adopted));
  expect(player.prepareSelection).not.toHaveBeenCalled();
});

test("control-enter on a provider row adds it to the queue", async () => {
  player.addToQueue.mockClear();
  await searchOne(adopted);
  const row = screen
    .getByRole("button", { name: "Play Provider song" })
    .closest("li") as HTMLElement;
  fireEvent.keyDown(row, { key: "Enter", ctrlKey: true });
  await waitFor(() => expect(player.addToQueue).toHaveBeenCalledWith(adopted));
});

test("a provider row without a ready source says so instead of queueing", async () => {
  player.addToQueue.mockClear();
  await searchOne({ ...adopted, available: false });
  fireEvent.click(
    screen.getByRole("button", { name: "Actions for Provider song" }),
  );
  fireEvent.click(
    await screen.findByRole("menuitem", { name: "Add to queue" }),
  );
  expect((await screen.findByRole("alert")).textContent).toContain(
    "No source is ready to queue",
  );
  expect(player.addToQueue).not.toHaveBeenCalled();
});
