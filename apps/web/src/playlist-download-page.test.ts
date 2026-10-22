import { createElement, type ReactNode } from "react";
import { act, render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import PlaylistPage from "./pages/PlaylistPage";
import { NoticeProvider } from "./shared/ui/Notices";
import { PlaybackProvider, usePlayer } from "./player/PlaybackProvider";
import type { Player } from "./player/model";
import type { PlaylistDownloadStatus, PlaylistPageProps } from "./shared/types";
import {
  copiedPlaylistPage,
  recordingA,
  recordingB,
} from "./fixtures/playlistPages";

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ href, children }: { href: string; children: ReactNode }) =>
    createElement("a", { href }, children),
}));

test("ready entries on a bounded status page remain explicitly partial across focus and timer checks", async () => {
  vi.useFakeTimers();
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const props: PlaylistPageProps = {
    ...copiedPlaylistPage,
    detail: {
      ...copiedPlaylistPage.detail,
      entries: Array.from({ length: 40 }, (_, index) => ({
        ...copiedPlaylistPage.detail.entries[0],
        entry_id: `00000000-0000-4000-8000-${String(index + 1).padStart(12, "0")}`,
        recording_id: index % 2 === 0 ? recordingA : recordingB,
        position: index,
        title: `Fixture track ${index + 1}`,
        inherited_title: `Fixture track ${index + 1}`,
        origin: {
          kind: "recording",
          provider_recording_id: index % 2 === 0 ? "provider/a" : "provider/b",
          occurrence_id: `fixture-${index + 1}`,
        },
      })),
    },
  };
  const path = `/api/v1/playlists/${props.detail.playlist.id}/download`;
  const retention = "c30c15ce-40dd-4b2a-8098-7d45d18d50c7";
  const ready: PlaylistDownloadStatus = {
    retention_id: retention,
    playlist_id: props.detail.playlist.id,
    host_device_id: "owned-download-host",
    desired_revision: 1,
    reconciled_revision: 1,
    entries: {
      items: props.detail.entries.slice(0, 32).map((entry) => ({
        entry_id: entry.entry_id,
        recording_id: entry.recording_id,
        status: "ready",
        source_id:
          entry.recording_id === recordingB
            ? "e055b9eb-1c5e-42b5-b0c6-6977f8187db8"
            : "b15b0ae6-e757-4f16-9150-8d6997f04474",
        error: null,
      })),
      next_cursor: `${retention}:1:${props.detail.entries[31].entry_id}`,
    },
  };
  const fetch = vi.fn(async (url: string, options?: RequestInit) => {
    if (url === path && options?.method === "GET") return Response.json(ready);
    throw new Error(
      "Playlist download checks must stay on the bounded first page",
    );
  });
  vi.stubGlobal("fetch", fetch);
  let player!: Player;
  function Page() {
    player = usePlayer();
    return createElement(PlaylistPage, props);
  }
  const view = render(
    createElement(
      NoticeProvider,
      null,
      createElement(PlaybackProvider, {
        account: props.user.id,
        children: createElement(Page),
      }),
    ),
  );
  await act(async () => {});
  const status = within(
    screen.getByRole("region", { name: "Playlist download" }),
  );
  const table = screen.getByRole("table", { name: "Playlist entries" });
  const rows = () =>
    within(table)
      .getAllByRole("row")
      .slice(1)
      .map((row) => row.textContent);
  const contents = rows();
  const recordingLinks = () =>
    within(table)
      .getAllByRole("link")
      .map((link) => link.getAttribute("href"));
  const originalLinks = props.detail.entries.map(
    (entry) => `/recordings/${entry.recording_id}`,
  );
  const playback = player.snapshot();
  expect(contents).toHaveLength(40);
  expect(recordingLinks()).toEqual(originalLinks);
  expect(status.getByText("32 of 32 checked entries saved.")).toBeTruthy();
  expect(status.getByText(/More entries have not been checked/)).toBeTruthy();
  expect(status.queryByText("32 of 32 entries saved.")).toBeNull();
  expect(
    status.queryByText(
      /playlist download complete|all (?:playlist )?entries (?:are )?(?:saved|downloaded)|available offline/i,
    ),
  ).toBeNull();
  const progress = status.getByRole("progressbar", {
    name: "Playlist download progress",
  });
  expect(progress.getAttribute("max")).toBe("32");
  expect(progress.getAttribute("value")).toBe("32");
  expect(fetch).toHaveBeenCalledTimes(1);
  expect(fetch.mock.calls[0]).toEqual([
    path,
    expect.objectContaining({
      method: "GET",
      credentials: "same-origin",
      headers: expect.objectContaining({ "x-hito-account": props.user.id }),
      signal: expect.any(AbortSignal),
    }),
  ]);

  await act(async () => {
    window.dispatchEvent(new Event("focus"));
  });
  expect(fetch).toHaveBeenCalledTimes(2);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(30_000);
  });
  expect(fetch).toHaveBeenCalledTimes(3);
  expect(
    fetch.mock.calls.map(([url, options]) => [url, options?.method]),
  ).toEqual([
    [path, "GET"],
    [path, "GET"],
    [path, "GET"],
  ]);
  expect(status.getByText("32 of 32 checked entries saved.")).toBeTruthy();
  expect(status.getByText(/More entries have not been checked/)).toBeTruthy();
  expect(status.queryByText("32 of 32 entries saved.")).toBeNull();
  expect(
    status.queryByText(
      /playlist download complete|all (?:playlist )?entries (?:are )?(?:saved|downloaded)|available offline/i,
    ),
  ).toBeNull();
  expect(screen.getByRole("table", { name: "Playlist entries" })).toBe(table);
  expect(rows()).toEqual(contents);
  expect(recordingLinks()).toEqual(originalLinks);
  expect(player.snapshot()).toEqual(playback);
  expect(player.queueSnapshot().total).toBe(0);
  expect(play).not.toHaveBeenCalled();
  expect(document.querySelector("audio")?.getAttribute("src")).toBeNull();
  view.unmount();
  expect(vi.getTimerCount()).toBe(0);
  expect(document.querySelectorAll("audio")).toHaveLength(0);
});
