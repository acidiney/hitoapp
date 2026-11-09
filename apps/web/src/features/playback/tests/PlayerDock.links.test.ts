import { createElement } from "react";
import { act, render, screen, waitFor } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { Playback } from "../playback";
import PlayerDock from "../components/PlayerDock";
import { ArtworkProvider } from "../../artwork/components/ArtworkProvider";
import type { Track } from "../model";

let player: Playback;
vi.mock("../components/PlaybackProvider", () => ({ usePlayer: () => player }));
vi.mock("@inertiajs/react", () => ({ router: { visit: vi.fn() } }));

const track: Track = {
  id: "link-recording",
  release_id: "link-release",
  title: "Linked title",
  artist: "Linked artist",
  duration_ms: 10000,
  available: true,
};

async function dock(current: Track, identity: unknown) {
  const fetch = vi.fn(async () =>
    identity === null
      ? Response.json({ error: "Not found" }, { status: 404 })
      : Response.json(identity),
  );
  vi.stubGlobal("fetch", fetch);
  const audio = document.createElement("audio");
  vi.spyOn(audio, "load").mockImplementation(() => {});
  vi.spyOn(audio, "pause").mockImplementation(() => {});
  vi.spyOn(audio, "play").mockImplementation(async () => {});
  player = new Playback(audio, async (id) => [{ id, ready: true }]);
  await act(async () => {
    await player.playQueue([current]);
  });
  render(
    createElement(ArtworkProvider, {
      account: "alice",
      children: createElement(PlayerDock, { account: "alice" }),
    }),
  );
  return fetch;
}

test("the track title links to its album and the artist links to the matched artist page", async () => {
  await dock(
    { ...track, id: "link-matched" },
    {
      recording_id: "link-matched",
      revision: 1,
      state: {
        status: "selected",
        group_id: "artist-group-1",
        association_id: "a",
        artist_id: "x",
      },
    },
  );
  const title = screen.getByRole("link", { name: "Linked title" });
  expect(title.getAttribute("href")).toBe("/albums/link-release");
  await waitFor(() =>
    expect(
      screen.getByRole("link", { name: "Linked artist" }).getAttribute("href"),
    ).toBe("/artist-groups/artist-group-1"),
  );
});

test("a track without a library album links its title to the track page and an unmatched artist to search", async () => {
  await dock(
    { ...track, id: "link-unmatched", release_id: null },
    {
      recording_id: "link-unmatched",
      revision: 0,
      state: { status: "unselected" },
    },
  );
  expect(
    screen.getByRole("link", { name: "Linked title" }).getAttribute("href"),
  ).toBe("/recordings/link-unmatched");
  expect(
    screen.getByRole("link", { name: "Linked artist" }).getAttribute("href"),
  ).toBe("/search?q=Linked%20artist");
});

test("an unreadable artist match keeps the search link", async () => {
  await dock({ ...track, id: "link-failed" }, null);
  expect(
    screen.getByRole("link", { name: "Linked artist" }).getAttribute("href"),
  ).toBe("/search?q=Linked%20artist");
});

test("a collaboration links each credited artist and the featured artist in the player bar", async () => {
  await dock(
    {
      ...track,
      id: "link-collab",
      artist: "Duo A & Duo B feat. Guest C",
      credited_artists: [
        { id: "member-a", name: "Duo A", role: "main", join: " & " },
        { id: "member-b", name: "Duo B", role: "main" },
        { id: "member-c", name: "Guest C", role: "featured" },
      ],
    },
    null,
  );
  for (const [name, id] of [
    ["Duo A", "member-a"],
    ["Duo B", "member-b"],
    ["Guest C", "member-c"],
  ])
    expect(screen.getByRole("link", { name }).getAttribute("href")).toBe(
      `/artists/${id}`,
    );
  const line = screen.getByRole("link", { name: "Duo B" }).closest("small");
  expect(line?.textContent).toBe("Duo A & Duo B feat. Guest C");
});
