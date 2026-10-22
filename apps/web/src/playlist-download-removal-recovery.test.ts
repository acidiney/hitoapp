import { createElement, type ReactNode } from "react";
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import PlaylistPage from "./pages/PlaylistPage";
import { NoticeProvider } from "./components/Notices";
import { PlaybackProvider, usePlayer } from "./player/PlaybackProvider";
import type { Player } from "./player/model";
import type { PlaylistDownloadStatus } from "./types";
import {
  copiedPlaylistPage as props,
  recordingA,
  recordingB,
} from "./fixtures/playlistPages";

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ href, children }: { href: string; children: ReactNode }) =>
    createElement("a", { href }, children),
}));

test("an unconfirmed removal retries its captured root while an independently replaced root survives", async () => {
  vi.useFakeTimers();
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const warnings = vi.spyOn(console, "warn").mockImplementation(() => {});
  const path = `/api/v1/playlists/${props.detail.playlist.id}/download`;
  const oldRoot = "794da822-37ed-410d-acd8-3678d913559f";
  const newRoot = "d11efb2a-14a9-4c08-a145-eb11f389b2d9";
  const removePath = `/api/v1/downloads/${oldRoot}`;
  const privateFailure = "private signed playlist download removal failure";
  function status(retention: string): PlaylistDownloadStatus {
    return {
      retention_id: retention,
      playlist_id: props.detail.playlist.id,
      host_device_id: "owned-download-host",
      desired_revision: 1,
      reconciled_revision: 1,
      entries: {
        items: props.detail.entries.map((entry) => {
          const ready =
            entry.recording_id !== null &&
            (retention === oldRoot || entry.recording_id === recordingB);
          return {
            entry_id: entry.entry_id,
            recording_id: entry.recording_id,
            status:
              entry.recording_id === null
                ? "unavailable"
                : ready
                  ? "ready"
                  : "queued",
            source_id: ready
              ? entry.recording_id === recordingB
                ? "da1deea2-4350-4cff-92fe-66822207d092"
                : "a386ccad-666b-4339-a740-d8fefb134bfe"
              : null,
            error: entry.recording_id === null ? "unavailable_item" : null,
          };
        }),
        next_cursor: null,
      },
    };
  }
  let serverRoot = oldRoot;
  let removals = 0;
  let finishRetry!: (response: Response) => void;
  const fetch = vi.fn(async (url: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    if (url === path && method === "GET")
      return Response.json(status(serverRoot));
    if (url === removePath && method === "DELETE") {
      removals++;
      if (removals === 1) {
        serverRoot = newRoot;
        throw new TypeError(privateFailure);
      }
      return new Promise<Response>((resolve) => {
        finishRetry = resolve;
      });
    }
    throw new Error("Unexpected request outside owned removal recovery");
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
  const remove = screen.getByRole("button", { name: "Remove download" });
  const table = screen.getByRole("table", { name: "Playlist entries" });
  const rows = () => within(table).getAllByRole("row").slice(1);
  const contents = rows().map((row) => row.textContent);
  const recordingLinks = () =>
    within(table)
      .getAllByRole("link")
      .map((link) => link.getAttribute("href"));
  const originalLinks = [recordingA, recordingB, recordingA].map(
    (recording) => `/recordings/${recording}`,
  );
  const playback = player.snapshot();
  const writes = () =>
    fetch.mock.calls.filter(([, options]) => options?.method !== "GET");
  expect(screen.getByText(/3 of 4 entries saved/)).toBeTruthy();
  expect(recordingLinks()).toEqual(originalLinks);
  await act(async () => {
    fireEvent.click(remove);
    fireEvent.click(remove);
  });
  const retry = screen.getByRole("button", { name: "Retry removal" });
  expect(writes()).toHaveLength(1);
  expect(serverRoot).toBe(newRoot);
  expect(screen.getByRole("alert").textContent).toBe(
    "Hito could not confirm removal. Retry removal to recover its result.",
  );
  expect(document.body.textContent).not.toContain(privateFailure);
  expect(screen.queryByRole("button", { name: "Download" })).toBeNull();
  const original = writes()[0];
  expect(original[0]).toBe(removePath);
  expect(original[1]?.signal?.aborted).toBe(false);
  const beforeRetry = fetch.mock.calls.length;
  await act(async () => {
    window.dispatchEvent(new Event("focus"));
    await vi.advanceTimersByTimeAsync(60_000);
  });
  expect(fetch).toHaveBeenCalledTimes(beforeRetry);
  expect(vi.getTimerCount()).toBe(0);
  expect(screen.getByRole("button", { name: "Retry removal" })).toBe(retry);
  expect(screen.getByText(/3 of 4 entries saved/)).toBeTruthy();

  act(() => {
    fireEvent.click(retry);
    fireEvent.click(retry);
  });
  expect(writes()).toHaveLength(2);
  const repeated = writes()[1];
  expect(repeated[0]).toBe(removePath);
  expect(repeated[1]).toMatchObject({
    method: "DELETE",
    credentials: "same-origin",
    headers: { "x-hito-account": props.user.id, "x-hito-request": "1" },
    signal: expect.any(AbortSignal),
  });
  expect(repeated[1]?.body).toBeUndefined();
  expect(repeated[1]?.signal).not.toBe(original[1]?.signal);
  expect(repeated[1]?.signal?.aborted).toBe(false);
  expect((retry as HTMLButtonElement).disabled).toBe(true);
  expect(fetch).toHaveBeenCalledTimes(beforeRetry + 1);
  expect(rows().map((row) => row.textContent)).toEqual(contents);
  expect(recordingLinks()).toEqual(originalLinks);
  expect(player.snapshot()).toEqual(playback);

  await act(async () => finishRetry(new Response(null, { status: 204 })));
  expect(screen.getByRole("button", { name: "Remove download" })).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Retry removal" })).toBeNull();
  expect(screen.queryByRole("button", { name: "Download" })).toBeNull();
  expect(screen.queryByRole("alert")).toBeNull();
  expect(screen.getByText(/1 of 4 entries saved/)).toBeTruthy();
  expect(
    screen
      .getByRole("progressbar", { name: "Playlist download progress" })
      .getAttribute("value"),
  ).toBe("1");
  expect(
    fetch.mock.calls.map(([url, options]) => [url, options?.method]),
  ).toEqual([
    [path, "GET"],
    [removePath, "DELETE"],
    [removePath, "DELETE"],
    [path, "GET"],
  ]);
  expect(serverRoot).toBe(newRoot);
  expect(screen.getByRole("table", { name: "Playlist entries" })).toBe(table);
  expect(rows().map((row) => row.textContent)).toEqual(contents);
  expect(recordingLinks()).toEqual(originalLinks);
  expect(player.snapshot()).toEqual(playback);
  expect(player.queueSnapshot().total).toBe(0);
  expect(play).not.toHaveBeenCalled();
  expect(document.querySelector("audio")?.getAttribute("src")).toBeNull();
  expect(JSON.stringify(warnings.mock.calls)).not.toContain(privateFailure);
  view.unmount();
  expect(vi.getTimerCount()).toBe(0);
  expect(document.querySelectorAll("audio")).toHaveLength(0);
});
