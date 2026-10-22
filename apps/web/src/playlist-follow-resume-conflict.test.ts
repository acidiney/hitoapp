import { createElement, type ReactNode } from "react";
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { router } from "@inertiajs/react";
import PlaylistPage from "./pages/PlaylistPage";
import { NoticeProvider } from "./shared/ui/Notices";
import { PlaybackProvider } from "./player/PlaybackProvider";
import { copiedPlaylistPage } from "./fixtures/playlistPages";
import { playlistConnection } from "./fixtures/playlistImports";
import type { components } from "./generated";
import type { PlaylistPageProps } from "./shared/types";

type Schema = components["schemas"];

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ children, href }: { children: ReactNode; href: string }) =>
    createElement("a", { href }, children),
}));

test("a definite Resume conflict requires a fresh review and a new operation with current approval pins", async () => {
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
      name: "Connection before the conflict",
    },
  };
  const service: Schema["PluginService"] = {
    id: origin.service_id,
    name: "Provider before the conflict",
    base_url: "https://reviewed-provider.invalid/before/",
    revision: 5,
  };
  const settings: Schema["PluginSettings"] = {
    credential_storage_ready: true,
    installations: [installation],
    services: [service],
  };
  const changedSettings: Schema["PluginSettings"] = {
    ...settings,
    installations: [
      {
        ...installation,
        revision: 9,
        manifest: {
          ...installation.manifest,
          name: "Connection after the conflict",
        },
      },
    ],
    services: [
      {
        ...service,
        name: "Provider after the conflict",
        base_url: "https://reviewed-provider.invalid/after/",
        revision: 6,
      },
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
  const firstReview: Schema["PlaylistFollow"] = { ...paused, generation: 4 };
  const freshReview: Schema["PlaylistFollow"] = { ...paused, generation: 5 };
  const rawConflict =
    "Stale approval at https://private-provider.invalid/?secret=private-value";
  let resolveApproval!: (response: Response) => void;
  const approvalResponse = new Promise<Response>((resolve) => {
    resolveApproval = resolve;
  });
  let currentSettings = settings;
  let currentStatus = firstReview;
  let statusReads = 0;
  let settingsReads = 0;
  let syncRequests = 0;
  const fetch = vi.fn((path: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    if (
      path === `/api/v1/playlists/${paused.playlist_id}/follow` &&
      method === "GET"
    ) {
      statusReads++;
      return Promise.resolve(
        Response.json(statusReads === 1 ? paused : currentStatus),
      );
    }
    if (path === "/api/v1/plugins" && method === "GET") {
      settingsReads++;
      return Promise.resolve(Response.json(currentSettings));
    }
    if (path === "/api/v1/sync" && method === "POST") {
      syncRequests++;
      if (syncRequests === 1) {
        currentSettings = changedSettings;
        currentStatus = freshReview;
        return Promise.resolve(
          Response.json({ error: rawConflict }, { status: 409 }),
        );
      }
      if (syncRequests === 2) return approvalResponse;
    }
    throw new Error("Unexpected request outside conflict approval recovery");
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
  const table = screen.getByRole("table", { name: "Playlist entries" });
  const entries = () =>
    within(table)
      .getAllByRole("row")
      .slice(1)
      .map((row) => row.textContent);
  const savedEntries = entries();
  expect(syncRequests).toBe(0);
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Review connection" }));
  });
  expect(settingsReads).toBe(1);
  expect(statusReads).toBe(2);
  expect(screen.getByText(installation.manifest.name)).toBeTruthy();
  expect(screen.getByText(service.base_url, { exact: true })).toBeTruthy();
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Resume updates" }));
  });
  expect(syncRequests).toBe(1);
  const posts = () =>
    fetch.mock.calls.filter(([path]) => path === "/api/v1/sync");
  const first: Schema["SyncBatch"] = JSON.parse(String(posts()[0][1]?.body));
  expect(first.operations).toHaveLength(1);
  expect(first.operations[0].action).toEqual({
    kind: "playlist_follow_resume",
    playlist_id: paused.playlist_id,
    expected_generation: firstReview.generation,
    installation_revision: installation.revision,
    service_revision: service.revision,
  } satisfies Schema["Action"]);
  const approval = within(
    screen.getByRole("region", { name: "Resume playlist updates" }),
  );
  expect(approval.getByRole("alert").textContent).toMatch(
    /Review the current connection again/,
  );
  expect(approval.getByRole("alert").textContent).not.toContain(rawConflict);
  expect(
    screen.queryByRole("button", { name: "Retry resume request" }),
  ).toBeNull();
  expect(screen.queryByRole("button", { name: "Resume updates" })).toBeNull();
  expect(screen.queryByText(installation.manifest.name)).toBeNull();
  const review = screen.getByRole("button", { name: "Review connection" });
  expect((review as HTMLButtonElement).disabled).toBe(false);
  expect(statusReads).toBe(2);
  expect(entries()).toEqual(savedEntries);
  await act(async () => {
    fireEvent.click(review);
  });
  expect(settingsReads).toBe(2);
  expect(statusReads).toBe(3);
  expect(approval.queryByRole("alert")).toBeNull();
  expect(
    screen.getByText(changedSettings.installations[0].manifest.name),
  ).toBeTruthy();
  expect(
    screen.getByText(changedSettings.services[0].base_url, { exact: true }),
  ).toBeTruthy();
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Resume updates" }));
  });
  expect(posts()).toHaveLength(2);
  const second: Schema["SyncBatch"] = JSON.parse(String(posts()[1][1]?.body));
  expect(second.device_id).toBe(first.device_id);
  expect(first.device_id.length).toBeGreaterThan(0);
  expect(second.operations).toHaveLength(1);
  expect(second.operations[0].id).not.toBe(first.operations[0].id);
  for (const batch of [first, second])
    expect(batch.operations[0].id).toMatch(
      /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
    );
  expect(second.operations[0].action).toEqual({
    kind: "playlist_follow_resume",
    playlist_id: paused.playlist_id,
    expected_generation: freshReview.generation,
    installation_revision: changedSettings.installations[0].revision,
    service_revision: changedSettings.services[0].revision,
  } satisfies Schema["Action"]);
  expect(posts()[1][1]).toMatchObject({
    credentials: "same-origin",
    headers: { "x-hito-account": props.user.id },
    signal: expect.any(AbortSignal),
  });
  expect(posts()[1][1]?.signal?.aborted).toBe(false);
  expect(statusReads).toBe(3);
  expect(entries()).toEqual(savedEntries);
  currentStatus = {
    ...freshReview,
    generation: 6,
    status: "ready",
    error: null,
  };
  await act(async () => {
    resolveApproval(
      Response.json({
        acknowledged: [second.operations[0].id],
      } satisfies Schema["SyncReceipt"]),
    );
  });
  expect(statusReads).toBe(4);
  expect(settingsReads).toBe(2);
  expect(syncRequests).toBe(2);
  expect(screen.getByText("Ready", { exact: true })).toBeTruthy();
  expect(
    screen.queryByRole("region", { name: "Resume playlist updates" }),
  ).toBeNull();
  expect(entries()).toEqual(savedEntries);
  expect(router.reload).not.toHaveBeenCalled();
  expect(router.visit).not.toHaveBeenCalled();
  expect(play).not.toHaveBeenCalled();
  const captured = JSON.stringify(logs.flatMap((sink) => sink.mock.calls));
  for (const privateValue of [
    rawConflict,
    origin.snapshot,
    service.base_url,
    changedSettings.services[0].base_url,
    props.detail.playlist.name,
  ])
    expect(captured).not.toContain(privateValue);
  const beforeUnmount = fetch.mock.calls.length;
  view.unmount();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(60_000);
  });
  expect(fetch).toHaveBeenCalledTimes(beforeUnmount);
});
