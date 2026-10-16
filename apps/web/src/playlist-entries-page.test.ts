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
import type { PlaylistEntry, PlaylistPageProps } from "./types";

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

const recordingA = "cd53993a-c582-42af-a67b-61280e39d3c2";
const recordingB = "eb6a9e9e-39fc-464f-bb2a-3ae17834e748";
const first: PlaylistEntry = {
  entry_id: "cf6d6fcb-1644-44b4-b49d-8ebd81e67250",
  recording_id: recordingA,
  position: 0,
  title: "Fixture A",
  inherited_title: "Fixture A",
  artist: "Fixture artist",
  inherited_artist: "Fixture artist",
  duration_ms: 12000,
  liked: false,
  available: true,
  origin: {
    kind: "recording",
    provider_recording_id: "provider/a",
    occurrence_id: "first-a",
  },
};
const unknown: PlaylistEntry = {
  ...first,
  entry_id: "dd56c1d9-4738-4993-8cfc-76a65cabf77d",
  recording_id: null,
  position: 1,
  title: "Unknown provider item",
  inherited_title: "Unknown provider item",
  artist: "",
  inherited_artist: "",
  duration_ms: 0,
  available: false,
  origin: {
    kind: "unknown",
    provider_recording_id: null,
    occurrence_id: "unknown",
  },
};
const props: PlaylistPageProps = {
  user: { id: "alice", username: "Alice", admin: false },
  errors: {},
  native_playback: false,
  libraries: [],
  playlists: [],
  detail: {
    playlist: {
      id: "eb0f0af1-dc27-4737-ad8a-928b8bf6a32e",
      name: "Copied selection",
      kind: "local",
    },
    origin: {
      provider: "org.hito.playlist-fixture",
      installation_id: "removed-installation",
      installation_revision: 3,
      service_id: "removed-service",
      service_revision: 1,
      provider_playlist_id: "canonical/selection",
      snapshot: "retained-snapshot",
      imported_at: 1,
      attribution: { label: "Curated by the fixture owner" },
    },
    entries: [
      first,
      unknown,
      {
        ...first,
        entry_id: "1813dcbd-9ce3-4b9d-bfb6-c70d3f4a564f",
        recording_id: recordingB,
        position: 2,
        title: "Fixture B",
        inherited_title: "Fixture B",
        origin: {
          kind: "recording",
          provider_recording_id: "provider/b",
          occurrence_id: "b",
        },
      },
      {
        ...first,
        entry_id: "b3ecda1c-e351-42d5-8916-0d0eb32ae081",
        position: 3,
        origin: {
          kind: "recording",
          provider_recording_id: "provider/a",
          occurrence_id: "last-a",
        },
      },
    ],
  },
};

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
