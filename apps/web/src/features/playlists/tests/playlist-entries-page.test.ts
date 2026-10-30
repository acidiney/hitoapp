import type { Schema } from "../../../shared/schema";
import { createElement, type ReactNode } from "react";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { router } from "@inertiajs/react";
import PlaylistPage from "../pages/PlaylistPage";
import { NoticeProvider } from "../../../shared/ui/Notices";
import {
  PlaybackProvider,
  usePlayer,
} from "../../playback/components/PlaybackProvider";
import type { Player } from "../../playback/model";

import {
  copiedPlaylistPage as props,
  recordingA,
  recordingB,
  unknown,
} from "../fixtures/playlistPages";

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

test("copied playlist keeps unknown occurrences out of recording actions and the real playback queue", async () => {
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const downloadPath = `/api/v1/playlists/${props.detail.playlist.id}/download`;
  const fetch = vi
    .fn()
    .mockImplementation(async (path: string, options: RequestInit) => {
      if (
        path === downloadPath &&
        options.method === "GET" &&
        new Headers(options.headers).get("x-hito-account") === props.user.id
      )
        return Response.json({ error: "Not found" }, { status: 404 });
      if (path === `/api/v1/recordings/${recordingA}/playback-sources`) {
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
      if (path === "/api/v1/sync") {
        const batch: Schema<"SyncBatch"> = JSON.parse(String(options.body));
        return Response.json({
          acknowledged: batch.operations.map((operation) => operation.id),
        });
      }
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
  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  const retained = within(rows[1]);
  expect(retained.queryAllByRole("link")).toHaveLength(0);
  expect(rows[1].querySelector("img")).toBeNull();
  expect(
    retained.queryByRole("button", { name: /Play|Like|Unlike|Details/ }),
  ).toBeNull();
  expect(retained.getAllByRole("button")).toHaveLength(3);
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
    downloadPath,
    `/api/v1/recordings/${recordingA}/playback-sources`,
    "/api/v1/sync",
  ]);
  expect(fetch.mock.calls[0]).toEqual([
    downloadPath,
    expect.objectContaining({
      method: "GET",
      credentials: "same-origin",
      headers: expect.objectContaining({ "x-hito-account": props.user.id }),
      signal: expect.any(AbortSignal),
    }),
  ]);
  expect(screen.getByText(/Curated by the fixture owner/)).toBeTruthy();
  expect(screen.getByRole("table", { name: "Playlist entries" })).toBe(table);
});
