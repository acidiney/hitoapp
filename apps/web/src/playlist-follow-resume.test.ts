import { createElement, type ReactNode } from "react";
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { router } from "@inertiajs/react";
import PlaylistPage from "./pages/PlaylistPage";
import { NoticeProvider } from "./components/Notices";
import { PlaybackProvider } from "./player/PlaybackProvider";
import { copiedPlaylistPage } from "./fixtures/playlistPages";
import { playlistConnection } from "./fixtures/playlistImports";
import type { components } from "./generated";
import type { PlaylistPageProps } from "./types";

type Schema = components["schemas"];

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ children, href }: { children: ReactNode; href: string }) =>
    createElement("a", { href }, children),
}));

function heldResponse() {
  let resolve!: (response: Response) => void;
  const response = new Promise<Response>((accept) => {
    resolve = accept;
  });
  return { response, resolve };
}

test("a paused playlist explicitly reviews its original connection and retries an unconfirmed resume unchanged", async () => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date("2026-10-17T03:00:00Z"));
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const logs = (["debug", "info", "warn", "error"] as const).map((level) =>
    vi.spyOn(console, level).mockImplementation(() => {}),
  );
  const origin = copiedPlaylistPage.detail.origin;
  if (!origin) throw new Error("The playlist fixture must have its origin");
  const props: PlaylistPageProps = {
    ...copiedPlaylistPage,
    detail: {
      ...copiedPlaylistPage.detail,
      playlist: { ...copiedPlaylistPage.detail.playlist, kind: "followed" },
      follow_content_revision: 1,
    },
  };
  const installation: Schema["PluginInstallation"] = {
    ...playlistConnection,
    id: origin.installation_id,
    service_id: origin.service_id,
    revision: 8,
    manifest: {
      ...playlistConnection.manifest,
      id: origin.provider,
      name: "Reviewed playlist connection",
      capabilities: ["playlists", "search"],
    },
  };
  const service: Schema["PluginService"] = {
    id: origin.service_id,
    name: "Reviewed provider service",
    base_url: "https://reviewed-provider.invalid/music/",
    revision: 5,
  };
  const settings: Schema["PluginSettings"] = {
    credential_storage_ready: true,
    installations: [
      {
        ...installation,
        id: "different-installation",
        service_id: "different-service",
        revision: 99,
        manifest: { ...installation.manifest, name: "Unrelated connection" },
      },
      installation,
    ],
    services: [
      {
        ...service,
        id: "different-service",
        name: "Unrelated provider service",
        base_url: "https://unrelated-provider.invalid/",
        revision: 99,
      },
      service,
    ],
  };
  const paused: Schema["PlaylistFollow"] = {
    playlist_id: props.detail.playlist.id,
    generation: 3,
    content_revision: 1,
    interval: "hourly",
    snapshot: origin.snapshot,
    last_refreshed_at: Math.floor(Date.now() / 1000),
    next_refresh_at: Math.floor(Date.now() / 1000) + 3600,
    status: "paused",
    error: "authority_changed",
  };
  const reviewedStatus: Schema["PlaylistFollow"] = {
    ...paused,
    generation: 4,
  };
  const resumed: Schema["PlaylistFollow"] = {
    ...reviewedStatus,
    generation: 5,
    status: "ready",
    error: null,
  };
  const statusPath = `/api/v1/playlists/${paused.playlist_id}/follow`;
  const reviewSettings = heldResponse();
  const reviewStatus = heldResponse();
  const retryResponse = heldResponse();
  const rawError =
    "Lost response from https://private-provider.invalid/?token=private-secret";
  let currentSettings = settings;
  let currentStatus = reviewedStatus;
  let statusReads = 0;
  let settingsReads = 0;
  let syncRequests = 0;
  const fetch = vi.fn((path: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    if (path === statusPath && method === "GET") {
      statusReads++;
      if (statusReads === 1) return Promise.resolve(Response.json(paused));
      if (statusReads === 2) return reviewStatus.response;
      return Promise.resolve(Response.json(currentStatus));
    }
    if (path === "/api/v1/plugins" && method === "GET") {
      settingsReads++;
      return settingsReads === 1
        ? reviewSettings.response
        : Promise.resolve(Response.json(currentSettings));
    }
    if (path === "/api/v1/sync" && method === "POST") {
      syncRequests++;
      if (syncRequests === 1) {
        currentStatus = resumed;
        return Promise.reject(new TypeError(rawError));
      }
      if (syncRequests === 2) return retryResponse.response;
    }
    throw new Error("Unexpected request outside reviewed playlist resume");
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
  await act(async () => {});
  expect(screen.getByText("Paused", { exact: true })).toBeTruthy();
  const table = screen.getByRole("table", { name: "Playlist entries" });
  const entries = () =>
    within(table)
      .getAllByRole("row")
      .slice(1)
      .map((row) => row.textContent);
  const originalEntries = entries();
  expect(originalEntries).toHaveLength(props.detail.entries.length);
  expect(syncRequests).toBe(0);
  expect(settingsReads).toBe(0);
  const review = screen.getByRole("button", { name: "Review connection" });
  await act(async () => {
    fireEvent.click(review);
  });
  expect(settingsReads).toBe(1);
  expect(syncRequests).toBe(0);
  expect(entries()).toEqual(originalEntries);
  await act(async () => reviewSettings.resolve(Response.json(settings)));
  expect(statusReads).toBe(2);
  const pendingApproval = screen.queryByRole("button", {
    name: "Resume updates",
  });
  expect(
    pendingApproval === null || (pendingApproval as HTMLButtonElement).disabled,
  ).toBe(true);
  await act(async () => reviewStatus.resolve(Response.json(reviewedStatus)));
  expect(screen.getByText(installation.manifest.name)).toBeTruthy();
  expect(screen.getByText(service.name)).toBeTruthy();
  expect(screen.getByText(service.base_url, { exact: true })).toBeTruthy();
  expect(screen.getByText(/playlists.*search/i)).toBeTruthy();
  expect(screen.queryByText("Unrelated connection")).toBeNull();
  expect(screen.queryByText("Unrelated provider service")).toBeNull();
  expect(syncRequests).toBe(0);
  for (const [, options] of fetch.mock.calls)
    expect(options).toMatchObject({
      credentials: "same-origin",
      headers: { "x-hito-account": props.user.id },
      signal: expect.any(AbortSignal),
    });
  const resume = screen.getByRole("button", { name: "Resume updates" });
  expect((resume as HTMLButtonElement).disabled).toBe(false);
  await act(async () => {
    fireEvent.click(resume);
  });
  expect(syncRequests).toBe(1);
  const posts = () =>
    fetch.mock.calls.filter(([path]) => path === "/api/v1/sync");
  const originalBody = String(posts()[0][1]?.body);
  const batch: Schema["SyncBatch"] = JSON.parse(originalBody);
  expect(batch).toEqual({
    device_id: expect.any(String),
    operations: [
      {
        id: expect.stringMatching(
          /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
        ),
        action: {
          kind: "playlist_follow_resume",
          playlist_id: paused.playlist_id,
          expected_generation: reviewedStatus.generation,
          installation_revision: installation.revision,
          service_revision: service.revision,
        } satisfies Schema["Action"],
      },
    ],
  });
  expect(batch.device_id.length).toBeGreaterThan(0);
  expect(statusReads).toBe(2);
  expect(screen.queryByText(rawError)).toBeNull();
  expect(entries()).toEqual(originalEntries);
  currentSettings = {
    ...settings,
    installations: [{ ...installation, revision: installation.revision + 1 }],
    services: [{ ...service, revision: service.revision + 1 }],
  };
  await act(async () => {
    window.dispatchEvent(new Event("focus"));
  });
  expect(statusReads).toBe(3);
  const retry = screen.getByRole("button", { name: "Retry resume request" });
  await act(async () => {
    fireEvent.click(retry);
    fireEvent.click(retry);
  });
  expect(posts()).toHaveLength(2);
  for (const [, options] of posts())
    expect(options).toMatchObject({
      method: "POST",
      credentials: "same-origin",
      headers: { "x-hito-account": props.user.id },
      body: originalBody,
      signal: expect.any(AbortSignal),
    });
  expect(posts()[1][1]?.signal).not.toBe(posts()[0][1]?.signal);
  expect(posts()[1][1]?.signal?.aborted).toBe(false);
  expect((retry as HTMLButtonElement).disabled).toBe(true);
  expect(entries()).toEqual(originalEntries);
  expect(statusReads).toBe(3);
  await act(async () => {
    retryResponse.resolve(
      Response.json({
        acknowledged: [batch.operations[0].id],
      } satisfies Schema["SyncReceipt"]),
    );
  });
  expect(statusReads).toBe(4);
  expect(screen.getByText("Ready", { exact: true })).toBeTruthy();
  expect(
    screen.queryByRole("button", { name: "Retry resume request" }),
  ).toBeNull();
  expect(entries()).toEqual(originalEntries);
  expect(posts()).toHaveLength(2);
  expect(router.reload).not.toHaveBeenCalled();
  expect(router.visit).not.toHaveBeenCalled();
  expect(play).not.toHaveBeenCalled();
  const captured = JSON.stringify(logs.flatMap((sink) => sink.mock.calls));
  for (const privateValue of [
    rawError,
    origin.snapshot,
    service.base_url,
    props.detail.playlist.name,
    ...props.detail.entries.map((entry) => entry.title),
  ])
    expect(captured).not.toContain(privateValue);
  const beforeUnmount = fetch.mock.calls.length;
  view.unmount();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(60_000);
  });
  expect(fetch).toHaveBeenCalledTimes(beforeUnmount);
});
