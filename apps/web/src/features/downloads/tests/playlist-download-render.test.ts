import { createElement, type ComponentProps, type ReactNode } from "react";
import { act, cleanup, render, screen, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import PlaylistPage from "../../playlists/pages/PlaylistPage";
import { NoticeProvider } from "../../../shared/ui/Notices";
import {
  PlaybackProvider,
  usePlayer,
} from "../../playback/components/PlaybackProvider";
import type { Player } from "../../playback/model";
import type {
  PlaylistDownloadStatus,
  PlaylistPageProps,
} from "../../../shared/types";
import {
  copiedPlaylistPage,
  recordingA,
  recordingB,
} from "../../playlists/fixtures/playlistPages";

const tableRenders = vi.hoisted(() => ({ count: 0 }));

vi.mock("../../../shared/ui/TrackTable", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("../../../shared/ui/TrackTable")>();
  return {
    ...actual,
    default: function CountedTrackTable(
      props: ComponentProps<typeof actual.default>,
    ) {
      tableRenders.count++;
      return createElement(actual.default, props);
    },
  };
});

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ href, children }: { href: string; children: ReactNode }) =>
    createElement("a", { href }, children),
}));

afterEach(() => {
  cleanup();
  tableRenders.count = 0;
});

test("download polling updates progress without rendering the unchanged playlist table again", async () => {
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
        entry_id: `20000000-0000-4000-8000-${String(index + 1).padStart(12, "0")}`,
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
  const retention = "ee172821-334f-41ed-8054-a6ef067a4010";
  function status(completed: boolean): PlaylistDownloadStatus {
    return {
      retention_id: retention,
      playlist_id: props.detail.playlist.id,
      host_device_id: "owned-download-host",
      desired_revision: 1,
      reconciled_revision: 1,
      entries: {
        items: props.detail.entries.slice(0, 32).map((entry) => {
          const ready = completed || entry.recording_id === recordingB;
          return {
            entry_id: entry.entry_id,
            recording_id: entry.recording_id,
            status: ready ? "ready" : "running",
            source_id: ready
              ? entry.recording_id === recordingB
                ? "8c482336-c912-41a5-9055-e51e10cc1623"
                : "ed9bf161-e1ee-4d55-a9de-e56b58e642da"
              : null,
            error: null,
          };
        }),
        next_cursor: `${retention}:1:${props.detail.entries[31].entry_id}`,
      },
    };
  }
  let finishPoll!: (response: Response) => void;
  const poll = new Promise<Response>((resolve) => {
    finishPoll = resolve;
  });
  let reads = 0;
  const fetch = vi.fn((url: string, options?: RequestInit) => {
    if (
      url === path &&
      options?.method === "GET" &&
      new Headers(options.headers).get("x-hito-account") === props.user.id
    ) {
      reads++;
      return reads === 1 ? Promise.resolve(Response.json(status(false))) : poll;
    }
    throw new Error("Unexpected request outside playlist download polling");
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
  const baseline = tableRenders.count;
  expect(baseline).toBeGreaterThan(0);
  expect(contents).toHaveLength(40);
  expect(recordingLinks()).toEqual(originalLinks);
  expect(screen.getByText("16 of 32 checked entries saved.")).toBeTruthy();
  expect(
    screen
      .getByRole("progressbar", { name: "Playlist download progress" })
      .getAttribute("value"),
  ).toBe("16");
  expect(fetch).toHaveBeenCalledTimes(1);

  await act(async () => {
    await vi.advanceTimersByTimeAsync(1000);
  });
  expect(fetch).toHaveBeenCalledTimes(2);
  await act(async () => finishPoll(Response.json(status(true))));
  expect(screen.getByText("32 of 32 checked entries saved.")).toBeTruthy();
  expect(
    screen
      .getByRole("progressbar", { name: "Playlist download progress" })
      .getAttribute("value"),
  ).toBe("32");
  expect(tableRenders.count).toBe(baseline);
  expect(
    fetch.mock.calls.map(([url, options]) => [url, options?.method]),
  ).toEqual([
    [path, "GET"],
    [path, "GET"],
  ]);
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
