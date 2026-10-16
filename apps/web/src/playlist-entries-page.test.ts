// @vitest-environment jsdom
import { createElement, type ReactNode } from "react";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { router } from "@inertiajs/react";
import PlaylistPage from "./pages/PlaylistPage";
import { NoticeProvider } from "./components/Notices";
import { PlaybackProvider, usePlayer } from "./player/PlaybackProvider";
import type { Player } from "./player/model";
import {
  copiedPlaylistPage as props,
  recordingA,
  recordingB,
  unknown,
} from "./fixtures/playlistPages";

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({
    children,
    href,
    ...attributes
  }: {
    children: ReactNode;
    href: string;
  }) => createElement("a", { href, ...attributes }, children),
}));

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  vi.clearAllMocks();
  localStorage.clear();
});

test("copied playlist keeps unknown occurrences out of recording actions and the real playback queue", async () => {
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const fetch = vi.fn().mockImplementation(async (path: string) => {
    if (path === `/api/v1/recordings/${recordingA}/sources`) {
      return new Response(
        JSON.stringify([
          {
            id: "fixture-source-a",
            recording_id: recordingA,
            provider: "fixture",
            device_id: "server",
            ready: true,
          },
        ]),
      );
    }
    if (path === "/api/v1/sync")
      return new Response(JSON.stringify({ acknowledged: [] }));
    throw new Error("Unexpected recording request");
  });
  vi.stubGlobal("fetch", fetch);
  let player!: Player;
  function Page() {
    player = usePlayer();
    return createElement(PlaylistPage, props);
  }
  render(
    createElement(
      NoticeProvider,
      null,
      createElement(PlaybackProvider, {
        account: props.user.id,
        children: createElement(Page),
      }),
    ),
  );
  const table = screen.getByRole("table");
  const rows = within(table).getAllByRole("row").slice(1);
  expect(rows).toHaveLength(4);
  expect(
    rows.map(
      (row) =>
        within(row).getByText(/Fixture A|Fixture B|Unknown provider item/)
          .textContent,
    ),
  ).toEqual(["Fixture A", "Unknown provider item", "Fixture B", "Fixture A"]);
  fireEvent.click(
    within(rows[3]).getByRole("button", { name: "Play Fixture A" }),
  );
  await waitFor(() => expect(player.queueSnapshot().total).toBe(3));
  expect(player.queueSnapshot().currentIndex).toBe(2);
  const queue = await player.queuePage(0, 10);
  expect(queue.entries.map((entry) => entry.recording.id)).toEqual([
    recordingA,
    recordingB,
    recordingA,
  ]);
  expect(new Set(queue.entries.map((entry) => entry.id)).size).toBe(3);
  const retained = within(rows[1]);
  expect(retained.queryAllByRole("link")).toHaveLength(0);
  expect(rows[1].querySelector("img")).toBeNull();
  expect(
    retained.queryByRole("button", { name: /Play|Like|Unlike|Details/ }),
  ).toBeNull();
  expect(retained.getAllByRole("button")).toHaveLength(1);
  expect(retained.getAllByRole("cell")[4].textContent?.trim()).toBe("—");
  fireEvent.click(
    retained.getByRole("button", {
      name: "Remove Unknown provider item from playlist",
    }),
  );
  await waitFor(() => expect(router.reload).toHaveBeenCalledTimes(1));
  const mutation = fetch.mock.calls.find(([path]) => path === "/api/v1/sync");
  expect(mutation).toBeDefined();
  expect(JSON.parse(mutation![1].body).operations).toEqual([
    {
      id: expect.any(String),
      action: {
        kind: "entry_remove",
        playlist_id: props.detail.playlist.id,
        entry_id: unknown.entry_id,
      },
    },
  ]);
  expect(fetch.mock.calls.map(([path]) => path)).toEqual([
    `/api/v1/recordings/${recordingA}/sources`,
    "/api/v1/sync",
  ]);
  expect(screen.getByText(/Curated by the fixture owner/)).toBeTruthy();
  expect(screen.getByRole("table", { name: "Playlist entries" })).toBe(table);
});
