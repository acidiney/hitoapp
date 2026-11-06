import type { Schema } from "../../../shared/schema";
import { createElement, type ReactNode } from "react";
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { router } from "@inertiajs/react";
import PlaylistPage from "../../playlists/pages/PlaylistPage";
import { NoticeProvider } from "../../../shared/ui/Notices";
import {
  PlaybackProvider,
  usePlayer,
} from "../../playback/components/PlaybackProvider";
import type { Player } from "../../playback/model";

import {
  copiedPlaylistPage,
  recordingA,
  recordingB,
} from "../../playlists/fixtures/playlistPages";

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ href, children }: { href: string; children: ReactNode }) =>
    createElement("a", { href }, children),
}));

test("retrying failed playlist downloads sends one fresh request and keeps the same retention without changing playback", async () => {
  vi.useFakeTimers();
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const props: Schema<"PlaylistPageProps"> = {
    ...copiedPlaylistPage,
    detail: {
      ...copiedPlaylistPage.detail,
      entries: copiedPlaylistPage.detail.entries.filter(
        (entry) => entry.recording_id !== null,
      ),
    },
  };
  const path = `/api/v1/playlists/${props.detail.playlist.id}/download`;
  const retention = "f8f80ea5-1a30-455c-b528-90ef3201bede";
  let repaired = false;
  function status(): Schema<"PlaylistDownloadStatus"> {
    return {
      retention_id: retention,
      playlist_id: props.detail.playlist.id,
      host_device_id: "owned-download-host",
      desired_revision: 1,
      reconciled_revision: 1,
      entries: {
        items: props.detail.entries.map((entry) => {
          const ready = repaired || entry.recording_id === recordingB;
          return {
            entry_id: entry.entry_id,
            recording_id: entry.recording_id,
            status: ready ? "ready" : "failed",
            source_id: ready
              ? entry.recording_id === recordingB
                ? "83bdd24c-ab84-4d2e-a490-4f6fe725334c"
                : "c4c4966f-563b-44e1-962c-756571197ac8"
              : null,
            error: ready ? null : "transfer_failed",
          };
        }),
        next_cursor: null,
      },
    };
  }
  let finishRetry!: (response: Response) => void;
  const fetch = vi.fn(async (url: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    if (
      url === path &&
      new Headers(options?.headers).get("x-hito-account") === props.user.id
    ) {
      if (method === "GET") return Response.json(status());
      if (method === "POST")
        return new Promise<Response>((resolve) => {
          finishRetry = (response) => {
            repaired = true;
            resolve(response);
          };
        });
    }
    throw new Error("Unexpected request outside explicit playlist retry");
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
  const retry = screen.getByRole("button", { name: "Retry failed downloads" });
  const remove = screen.getByRole("button", { name: "Remove download" });
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
      .map((link) => link.getAttribute("href"))
      .filter((href) => href?.startsWith("/recordings/"));
  const originalLinks = [recordingA, recordingB, recordingA].map(
    (recording) => `/recordings/${recording}`,
  );
  const playback = player.snapshot();
  expect(contents).toHaveLength(3);
  expect(recordingLinks()).toEqual(originalLinks);
  expect(screen.getByText("1 of 3 entries saved.")).toBeTruthy();
  expect(screen.getByText(/2 entries could not be downloaded/)).toBeTruthy();
  expect(fetch).toHaveBeenCalledTimes(1);
  expect(fetch.mock.calls[0][1]?.method).toBe("GET");
  expect(play).not.toHaveBeenCalled();
  expect(document.querySelector("audio")?.getAttribute("src")).toBeNull();

  act(() => {
    fireEvent.click(retry);
    fireEvent.click(retry);
  });
  const posts = fetch.mock.calls.filter(
    ([, options]) => options?.method === "POST",
  );
  expect(posts).toHaveLength(1);
  expect(posts[0][0]).toBe(path);
  expect(posts[0][1]).toMatchObject({
    credentials: "same-origin",
    headers: { "x-hito-account": props.user.id, "x-hito-request": "1" },
    signal: expect.any(AbortSignal),
  });
  const request = JSON.parse(String(posts[0][1]?.body));
  expect(request).toEqual({
    request_id: expect.stringMatching(
      /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
    ),
  });
  expect(posts[0][1]?.signal?.aborted).toBe(false);
  expect((retry as HTMLButtonElement).disabled).toBe(true);
  expect((remove as HTMLButtonElement).disabled).toBe(true);
  expect(rows()).toEqual(contents);
  expect(recordingLinks()).toEqual(originalLinks);
  expect(player.snapshot()).toEqual(playback);

  await act(async () =>
    finishRetry(
      Response.json(
        {
          request_id: request.request_id,
          retention_id: retention,
          playlist_id: props.detail.playlist.id,
        },
        { status: 202 },
      ),
    ),
  );
  expect(screen.getByText("3 of 3 entries saved.")).toBeTruthy();
  expect(
    screen
      .getByRole("progressbar", { name: "Playlist download progress" })
      .getAttribute("value"),
  ).toBe("3");
  expect(
    screen.queryByRole("button", { name: "Retry failed downloads" }),
  ).toBeNull();
  expect(screen.queryByRole("button", { name: "Download" })).toBeNull();
  expect(
    (
      screen.getByRole("button", {
        name: "Remove download",
      }) as HTMLButtonElement
    ).disabled,
  ).toBe(false);
  expect(screen.queryByText(/entries could not be downloaded/)).toBeNull();
  expect(
    fetch.mock.calls.map(([url, options]) => [url, options?.method]),
  ).toEqual([
    [path, "GET"],
    [path, "POST"],
    [path, "GET"],
  ]);
  expect(
    fetch.mock.calls.filter(([, options]) => options?.method === "DELETE"),
  ).toHaveLength(0);
  expect(screen.getByRole("table", { name: "Playlist entries" })).toBe(table);
  expect(rows()).toEqual(contents);
  expect(recordingLinks()).toEqual(originalLinks);
  expect(player.snapshot()).toEqual(playback);
  expect(player.queueSnapshot().total).toBe(0);
  expect(router.reload).not.toHaveBeenCalled();
  expect(router.visit).not.toHaveBeenCalled();
  expect(play).not.toHaveBeenCalled();
  expect(document.querySelector("audio")?.getAttribute("src")).toBeNull();
  view.unmount();
  expect(vi.getTimerCount()).toBe(0);
  expect(document.querySelectorAll("audio")).toHaveLength(0);
});
