import { createElement, type ReactNode } from "react";
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { router } from "@inertiajs/react";
import PlaylistPage from "./PlaylistPage";
import { NoticeProvider } from "../shared/ui/Notices";
import { PlaybackProvider, usePlayer } from "../player/PlaybackProvider";
import type { Player } from "../player/model";
import type { PlaylistDownloadStatus, PlaylistPageProps } from "../shared/types";
import {
  copiedPlaylistPage,
  recordingA,
  recordingB,
} from "../fixtures/playlistPages";

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ href, children }: { href: string; children: ReactNode }) =>
    createElement("a", { href }, children),
}));

test("a lost failed-download retry acknowledgement recovers only its exact new request before checking the same retention", async () => {
  vi.useFakeTimers();
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const warnings = vi.spyOn(console, "warn").mockImplementation(() => {});
  const props: PlaylistPageProps = {
    ...copiedPlaylistPage,
    detail: {
      ...copiedPlaylistPage.detail,
      entries: copiedPlaylistPage.detail.entries.filter(
        (entry) => entry.recording_id !== null,
      ),
    },
  };
  const path = `/api/v1/playlists/${props.detail.playlist.id}/download`;
  const retention = "9ec5e26d-e1bb-47cf-9b4f-d6a68dce4038";
  const privateFailure = "private signed playlist retry acknowledgement loss";
  let accepted = false;
  function status(): PlaylistDownloadStatus {
    return {
      retention_id: retention,
      playlist_id: props.detail.playlist.id,
      host_device_id: "owned-download-host",
      desired_revision: 1,
      reconciled_revision: 1,
      entries: {
        items: props.detail.entries.map((entry) => {
          const ready = accepted || entry.recording_id === recordingB;
          return {
            entry_id: entry.entry_id,
            recording_id: entry.recording_id,
            status: ready ? "ready" : "failed",
            source_id: ready
              ? entry.recording_id === recordingB
                ? "5b6d3738-80ea-4b50-99e3-47680960490f"
                : "066cd75b-9640-4b2b-a945-32604e2593aa"
              : null,
            error: ready ? null : "transfer_failed",
          };
        }),
        next_cursor: null,
      },
    };
  }
  let posts = 0;
  let finishRecovery!: (response: Response) => void;
  const fetch = vi.fn(async (url: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    if (
      url === path &&
      new Headers(options?.headers).get("x-hito-account") === props.user.id
    ) {
      if (method === "GET") return Response.json(status());
      if (method === "POST") {
        posts++;
        accepted = true;
        if (posts === 1) throw new TypeError(privateFailure);
        return new Promise<Response>((resolve) => {
          finishRecovery = resolve;
        });
      }
    }
    throw new Error(
      "Unexpected request outside failed playlist retry recovery",
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
  const retryFailed = screen.getByRole("button", {
    name: "Retry failed downloads",
  });
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
  const originalLinks = [recordingA, recordingB, recordingA].map(
    (recording) => `/recordings/${recording}`,
  );
  const playback = player.snapshot();
  const writes = () =>
    fetch.mock.calls.filter(([, options]) => options?.method === "POST");
  expect(recordingLinks()).toEqual(originalLinks);
  expect(fetch).toHaveBeenCalledTimes(1);
  await act(async () => {
    fireEvent.click(retryFailed);
    fireEvent.click(retryFailed);
  });
  const recover = screen.getByRole("button", {
    name: "Retry download request",
  });
  expect(accepted).toBe(true);
  expect(writes()).toHaveLength(1);
  const original = writes()[0];
  const originalBody = String(original[1]?.body);
  const request = JSON.parse(originalBody);
  expect(request).toEqual({
    request_id: expect.stringMatching(
      /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
    ),
  });
  expect(original[1]).toMatchObject({
    method: "POST",
    credentials: "same-origin",
    headers: { "x-hito-account": props.user.id, "x-hito-request": "1" },
    signal: expect.any(AbortSignal),
  });
  expect(original[1]?.signal?.aborted).toBe(false);
  expect(screen.getByRole("alert").textContent).toBe(
    "Hito could not confirm the download request. Retry the same request to recover its result.",
  );
  expect(document.body.textContent).not.toContain(privateFailure);
  expect((retryFailed as HTMLButtonElement).disabled).toBe(true);
  expect(screen.queryByRole("button", { name: "Remove download" })).toBeNull();
  expect(screen.queryByRole("button", { name: "Download" })).toBeNull();
  const beforeRecovery = fetch.mock.calls.length;
  await act(async () => {
    fireEvent.click(retryFailed);
    window.dispatchEvent(new Event("focus"));
    await vi.advanceTimersByTimeAsync(60_000);
  });
  expect(fetch).toHaveBeenCalledTimes(beforeRecovery);
  expect(vi.getTimerCount()).toBe(0);
  expect(screen.getByText("1 of 3 entries saved.")).toBeTruthy();
  expect(screen.queryByText("3 of 3 entries saved.")).toBeNull();
  expect(rows()).toEqual(contents);
  expect(recordingLinks()).toEqual(originalLinks);
  expect(player.snapshot()).toEqual(playback);

  act(() => {
    fireEvent.click(recover);
    fireEvent.click(recover);
  });
  expect(writes()).toHaveLength(2);
  const repeated = writes()[1];
  expect(repeated[0]).toBe(path);
  expect(repeated[1]?.body).toBe(originalBody);
  expect(repeated[1]).toMatchObject({
    method: "POST",
    credentials: "same-origin",
    headers: { "x-hito-account": props.user.id, "x-hito-request": "1" },
    signal: expect.any(AbortSignal),
  });
  expect(repeated[1]?.signal).not.toBe(original[1]?.signal);
  expect(repeated[1]?.signal?.aborted).toBe(false);
  expect((recover as HTMLButtonElement).disabled).toBe(true);
  expect((retryFailed as HTMLButtonElement).disabled).toBe(true);
  expect(fetch).toHaveBeenCalledTimes(beforeRecovery + 1);
  await act(async () =>
    finishRecovery(
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
  expect(screen.getByRole("button", { name: "Remove download" })).toBeTruthy();
  expect(
    screen.queryByRole("button", { name: "Retry failed downloads" }),
  ).toBeNull();
  expect(
    screen.queryByRole("button", { name: "Retry download request" }),
  ).toBeNull();
  expect(screen.queryByRole("button", { name: "Download" })).toBeNull();
  expect(screen.queryByRole("alert")).toBeNull();
  expect(
    fetch.mock.calls.map(([url, options]) => [url, options?.method]),
  ).toEqual([
    [path, "GET"],
    [path, "POST"],
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
  expect(JSON.stringify(warnings.mock.calls)).not.toContain(privateFailure);
  view.unmount();
  expect(vi.getTimerCount()).toBe(0);
  expect(document.querySelectorAll("audio")).toHaveLength(0);
});
