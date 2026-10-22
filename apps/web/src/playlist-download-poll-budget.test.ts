import { act, renderHook } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { usePlaylistDownload } from "./hooks/usePlaylistDownload";
import type { PlaylistDownloadStatus } from "./shared/types";

test("unchanged busy status exhausts fast polling and observed progress reopens its budget", async () => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "performance"] });
  const account = "alice";
  const playlist = "5a5b413a-c67c-4880-8208-0f9ee3591304";
  const retention = "eb773cc6-a419-4564-a567-6b1c5050e145";
  const path = `/api/v1/playlists/${playlist}/download`;
  let progressed = false;
  function status(): PlaylistDownloadStatus {
    return {
      retention_id: retention,
      playlist_id: playlist,
      host_device_id: "owned-download-host",
      desired_revision: 1,
      reconciled_revision: 1,
      entries: {
        items: [
          {
            entry_id: "1f57c0a3-de4c-4793-8c48-a3c9decf804b",
            recording_id: "0e2c24db-03e3-4032-aa6d-8f08bd21bccc",
            status: progressed ? "ready" : "running",
            source_id: progressed
              ? "6a7c53bb-708a-4483-b06d-b7727e10557a"
              : null,
            error: null,
          },
          {
            entry_id: "b47d6dca-c0b0-4d09-963d-3c83a3af3683",
            recording_id: "a9443af1-9fc6-4b19-a0c7-91682c0175ec",
            status: "queued",
            source_id: null,
            error: null,
          },
        ],
        next_cursor: null,
      },
    };
  }
  const fetch = vi.fn(async (url: string, options?: RequestInit) => {
    if (
      url === path &&
      options?.method === "GET" &&
      new Headers(options.headers).get("x-hito-account") === account
    )
      return Response.json(status());
    throw new Error(
      "Unexpected request outside bounded playlist status polling",
    );
  });
  vi.stubGlobal("fetch", fetch);
  const { result, unmount } = renderHook(() =>
    usePlaylistDownload(account, playlist),
  );
  await act(async () => {});
  expect(fetch).toHaveBeenCalledTimes(1);
  expect(
    result.current.download?.entries.items.map((entry) => entry.status),
  ).toEqual(["running", "queued"]);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(1000);
  });
  expect(fetch).toHaveBeenCalledTimes(2);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(29_000);
  });
  const afterFastWindow = fetch.mock.calls.length;
  await act(async () => {
    await vi.advanceTimersByTimeAsync(60_000);
  });
  const laterChecks = fetch.mock.calls.length - afterFastWindow;
  expect(laterChecks).toBeLessThanOrEqual(3);
  expect(laterChecks).toBeGreaterThanOrEqual(2);
  expect(
    result.current.download?.entries.items.map((entry) => entry.status),
  ).toEqual(["running", "queued"]);

  progressed = true;
  const beforeProgress = fetch.mock.calls.length;
  await act(async () => {
    await vi.advanceTimersToNextTimerAsync();
  });
  expect(fetch).toHaveBeenCalledTimes(beforeProgress + 1);
  expect(
    result.current.download?.entries.items.map((entry) => entry.status),
  ).toEqual(["ready", "queued"]);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(1000);
  });
  expect(fetch).toHaveBeenCalledTimes(beforeProgress + 2);
  expect(
    fetch.mock.calls.every(
      ([url, options]) =>
        url === path &&
        options?.method === "GET" &&
        new Headers(options.headers).get("x-hito-account") === account,
    ),
  ).toBe(true);
  expect(fetch.mock.calls[0][1]).toMatchObject({
    credentials: "same-origin",
    headers: { "x-hito-account": account, "x-hito-request": "1" },
    signal: expect.any(AbortSignal),
  });
  expect(
    fetch.mock.calls.filter(([, options]) => options?.method === "POST"),
  ).toHaveLength(0);
  expect(result.current.pending).toBe(false);
  expect(result.current.checking).toBe(false);
  expect(result.current.error).toBe("");
  expect(result.current.statusError).toBe("");
  const beforeUnmount = fetch.mock.calls.length;
  unmount();
  expect(vi.getTimerCount()).toBe(0);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(60_000);
    window.dispatchEvent(new Event("focus"));
  });
  expect(fetch).toHaveBeenCalledTimes(beforeUnmount);
  expect(vi.getTimerCount()).toBe(0);
});
