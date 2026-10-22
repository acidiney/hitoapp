import { createElement, type ReactNode } from "react";
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import PlaylistPage from "../../../pages/PlaylistPage";
import { NoticeProvider } from "../../../shared/ui/Notices";
import { PlaybackProvider } from "../../../player/PlaybackProvider";
import { copiedPlaylistPage, recordingB } from "../../../fixtures/playlistPages";
import type { PlaylistPageProps } from "../../../shared/types";

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ href, children }: { href: string; children: ReactNode }) =>
    createElement("a", { href }, children),
}));

test("playlist download starts only on request and counts every retained entry without starting playback", async () => {
  vi.useFakeTimers();
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const recordingU = "71b4c635-c6c4-45ba-a0b2-ac8bd18d036b";
  const props: PlaylistPageProps = {
    ...copiedPlaylistPage,
    detail: {
      ...copiedPlaylistPage.detail,
      entries: copiedPlaylistPage.detail.entries.map((entry) =>
        entry.recording_id !== null
          ? entry
          : {
              ...entry,
              recording_id: recordingU,
              available: false,
              title: "Waiting provider track",
              inherited_title: "Waiting provider track",
              origin: {
                kind: "recording",
                provider_recording_id: "provider/u",
                occurrence_id: "u",
              },
            },
      ),
    },
  };
  const path = `/api/v1/playlists/${props.detail.playlist.id}/download`;
  const retention = "fe8d8202-a7d2-4a17-b3c8-d471a00b5823";
  const source = "7bb61473-336e-42a2-9c72-c02b95307716";
  let accepted = false;
  let completed = false;
  let finishCreate!: (response: Response) => void;
  const fetch = vi.fn(async (url: string, options?: RequestInit) => {
    if (url !== path)
      throw new Error(
        "Unexpected request outside the playlist download workflow",
      );
    const method = options?.method ?? "GET";
    if (method === "GET") {
      if (!accepted)
        return Response.json({ error: "Not found" }, { status: 404 });
      return Response.json({
        retention_id: retention,
        playlist_id: props.detail.playlist.id,
        host_device_id: "owned-download-host",
        desired_revision: 1,
        reconciled_revision: 1,
        entries: {
          items: props.detail.entries.map((entry) => ({
            entry_id: entry.entry_id,
            recording_id: entry.recording_id,
            status:
              entry.recording_id === recordingU
                ? "waiting_for_source"
                : entry.recording_id === recordingB && !completed
                  ? "running"
                  : "ready",
            source_id:
              entry.recording_id !== null &&
              entry.recording_id !== recordingU &&
              (entry.recording_id !== recordingB || completed)
                ? source
                : null,
            error: null,
          })),
          next_cursor: null,
        },
      });
    }
    if (method === "POST")
      return new Promise<Response>((resolve) => {
        finishCreate = (response) => {
          accepted = true;
          resolve(response);
        };
      });
    throw new Error(
      "Playlist download must not mutate entries or start playback",
    );
  });
  vi.stubGlobal("fetch", fetch);
  const view = render(
    createElement(
      NoticeProvider,
      null,
      createElement(PlaybackProvider, {
        account: props.user.id,
        children: createElement(PlaylistPage, props),
      }),
    ),
  );
  const download = screen.getByRole("button", { name: "Download" });
  const table = screen.getByRole("table", { name: "Playlist entries" });
  const entryRows = within(table)
    .getAllByRole("row")
    .slice(1)
    .map((row) => row.textContent);
  await act(async () => {});
  expect(fetch.mock.calls).toEqual([
    [
      path,
      expect.objectContaining({
        method: "GET",
        credentials: "same-origin",
        headers: expect.objectContaining({ "x-hito-account": props.user.id }),
        signal: expect.any(AbortSignal),
      }),
    ],
  ]);
  expect(play).not.toHaveBeenCalled();
  expect(document.querySelector("audio")?.getAttribute("src")).toBeNull();

  act(() => {
    fireEvent.click(download);
    fireEvent.click(download);
  });
  const posts = fetch.mock.calls.filter(
    ([, options]) => options?.method === "POST",
  );
  expect(posts).toHaveLength(1);
  const creation = posts[0];
  expect(creation[0]).toBe(path);
  expect(creation[1]).toMatchObject({
    credentials: "same-origin",
    headers: { "x-hito-account": props.user.id, "x-hito-request": "1" },
    signal: expect.any(AbortSignal),
  });
  const request = JSON.parse(String(creation[1]?.body));
  expect(request).toEqual({
    request_id: expect.stringMatching(
      /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
    ),
  });
  expect(creation[1]?.signal?.aborted).toBe(false);
  expect((download as HTMLButtonElement).disabled).toBe(true);
  await act(async () =>
    finishCreate(
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
  const status = within(
    screen.getByRole("region", { name: "Playlist download" }),
  );
  const progress = status.getByRole("progressbar", {
    name: "Playlist download progress",
  });
  expect(progress.getAttribute("max")).toBe("4");
  expect(progress.getAttribute("value")).toBe("2");
  expect(status.getByText(/2 of 4 entries saved/)).toBeTruthy();
  expect(status.getByText(/1 entry waiting for a source/)).toBeTruthy();
  expect(status.getByText(/copies are saved on this server/i)).toBeTruthy();
  expect(status.getByText(/browser.*needs.*connection.*server/i)).toBeTruthy();
  expect(status.queryByText(/saved on this device/i)).toBeNull();
  expect(
    within(table)
      .getAllByRole("row")
      .slice(1)
      .map((row) => row.textContent),
  ).toEqual(entryRows);

  completed = true;
  await act(async () => {
    await vi.advanceTimersByTimeAsync(1000);
  });
  const currentProgress = status.getByRole("progressbar", {
    name: "Playlist download progress",
  });
  expect(currentProgress.getAttribute("max")).toBe("4");
  expect(currentProgress.getAttribute("value")).toBe("3");
  expect(status.getByText(/3 of 4 entries saved/)).toBeTruthy();
  expect(status.getByText(/1 entry waiting for a source/)).toBeTruthy();
  expect(screen.getByRole("table", { name: "Playlist entries" })).toBe(table);
  expect(
    fetch.mock.calls.filter(([, options]) => options?.method !== "GET"),
  ).toHaveLength(1);
  expect(play).not.toHaveBeenCalled();
  expect(document.querySelector("audio")?.getAttribute("src")).toBeNull();
  view.unmount();
  expect(vi.getTimerCount()).toBe(0);
  expect(document.querySelectorAll("audio")).toHaveLength(0);
});
