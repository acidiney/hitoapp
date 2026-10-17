// @vitest-environment jsdom
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { usePlaylistDownload } from "./hooks/usePlaylistDownload";
import type { PlaylistDownloadStatus } from "./types";

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  vi.clearAllMocks();
  localStorage.clear();
});

test("a rendered removal action cannot retarget a root discovered by a later status read", async () => {
  vi.useFakeTimers();
  const account = "alice";
  const playlist = "fc48db53-0b7e-48ad-a3f3-b397a886f213";
  const oldRoot = "c278bb6b-52d3-4029-bb70-6577e08a0192";
  const newRoot = "969ca8e9-8e35-46d9-8104-b99169fec8b3";
  const path = `/api/v1/playlists/${playlist}/download`;
  const oldRemovalPath = `/api/v1/downloads/${oldRoot}`;
  const newRemovalPath = `/api/v1/downloads/${newRoot}`;
  function status(retention: string): PlaylistDownloadStatus {
    return {
      retention_id: retention,
      playlist_id: playlist,
      host_device_id: "owned-download-host",
      desired_revision: 1,
      reconciled_revision: 1,
      entries: {
        items: [
          {
            entry_id: "0d6a8a4c-8422-4db2-96fa-d6d442bc0fbc",
            recording_id: "af39ae6b-e1eb-49c6-a451-ae6d711282cd",
            source_id: "a7316011-170d-4dd7-9ad0-c4517a7d3ab7",
            status: "ready",
            error: null,
          },
        ],
        next_cursor: null,
      },
    };
  }
  let serverRoot: string | null = oldRoot;
  const fetch = vi.fn(async (url: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    const owner = new Headers(options?.headers).get("x-hito-account");
    if (owner !== account)
      throw new Error("Playlist download requests must keep their account");
    if (url === path && method === "GET")
      return serverRoot === null
        ? Response.json({ error: "Not found" }, { status: 404 })
        : Response.json(status(serverRoot));
    if (method === "DELETE" && [oldRemovalPath, newRemovalPath].includes(url)) {
      const target = url === oldRemovalPath ? oldRoot : newRoot;
      if (serverRoot === target) serverRoot = null;
      return new Response(null, { status: 204 });
    }
    throw new Error("Unexpected request outside displayed playlist removal");
  });
  vi.stubGlobal("fetch", fetch);
  const { result, unmount } = renderHook(() =>
    usePlaylistDownload(account, playlist),
  );
  await act(async () => {});
  expect(result.current.download?.retention_id).toBe(oldRoot);
  const removeDisplayedRoot = result.current.remove;

  serverRoot = newRoot;
  await act(async () => {
    window.dispatchEvent(new Event("focus"));
  });
  expect(result.current.download?.retention_id).toBe(newRoot);
  expect(fetch).toHaveBeenCalledTimes(2);
  const currentView = result.current.download;
  await act(async () => {
    await removeDisplayedRoot();
  });
  const removals = fetch.mock.calls.filter(
    ([, options]) => options?.method === "DELETE",
  );
  expect(removals.map(([url]) => url)).not.toContain(newRemovalPath);
  expect(removals.length).toBeLessThanOrEqual(1);
  for (const removal of removals) {
    expect(removal[0]).toBe(oldRemovalPath);
    expect(removal[1]).toMatchObject({
      method: "DELETE",
      credentials: "same-origin",
      headers: { "x-hito-account": account, "x-hito-request": "1" },
      signal: expect.any(AbortSignal),
    });
    expect(removal[1]?.body).toBeUndefined();
    expect(removal[1]?.signal?.aborted).toBe(false);
  }
  expect(
    fetch.mock.calls.every(([url, options]) =>
      options?.method === "GET"
        ? url === path
        : options?.method === "DELETE" && url === oldRemovalPath,
    ),
  ).toBe(true);
  expect(
    fetch.mock.calls.filter(([, options]) => options?.method === "POST"),
  ).toHaveLength(0);
  expect(serverRoot).toBe(newRoot);
  expect(result.current.download).toEqual(currentView);
  expect(result.current.requested).toBe(true);
  expect(result.current.pending).toBe(false);
  expect(result.current.checking).toBe(false);
  expect(result.current.retryKind).toBeNull();
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
