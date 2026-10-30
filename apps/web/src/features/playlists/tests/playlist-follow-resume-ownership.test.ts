import type { Schema } from "../../../shared/schema";
import { createElement, type ReactNode } from "react";
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { router } from "@inertiajs/react";
import PlaylistPage from "../pages/PlaylistPage";
import { NoticeProvider } from "../../../shared/ui/Notices";
import { PlaybackProvider } from "../../playback/components/PlaybackProvider";
import { copiedPlaylistPage } from "../fixtures/playlistPages";
import { playlistConnection } from "../fixtures/playlistImports";
import type { components } from "../../../generated";

import { heldResponse } from "../../../test/async";

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ children, href }: { children: ReactNode; href: string }) =>
    createElement("a", { href }, children),
}));

function page(props: Schema<"PlaylistPageProps">) {
  return createElement(
    NoticeProvider,
    null,
    createElement(PlaybackProvider, {
      account: props.user.id,
      children: createElement(PlaylistPage, props),
    }),
  );
}

test.each(["account", "playlist"] as const)(
  "%s changes cancel Resume review and submission without applying late responses to the new owner",
  async (change) => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-10-17T03:00:00Z"));
    vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
    vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
    const play = vi
      .spyOn(HTMLMediaElement.prototype, "play")
      .mockResolvedValue();
    const origin = copiedPlaylistPage.detail.origin;
    if (!origin) throw new Error("The playlist fixture must have its origin");
    const accounts = ["alice", "bob", "carol"];
    const playlists = [
      copiedPlaylistPage.detail.playlist.id,
      "88fefb77-e103-4c73-8e6b-bdf6ee867184",
      "4aeebd10-09bf-4988-9012-705dbd78c1d2",
    ];
    const pages: Schema<"PlaylistPageProps">[] = accounts.map(
      (account, index) => ({
        ...copiedPlaylistPage,
        user:
          change === "account"
            ? { ...copiedPlaylistPage.user, id: account, username: account }
            : copiedPlaylistPage.user,
        detail: {
          ...copiedPlaylistPage.detail,
          playlist: {
            ...copiedPlaylistPage.detail.playlist,
            id: change === "playlist" ? playlists[index] : playlists[0],
            name: `Owned selection ${index + 1}`,
            kind: "followed",
          },
          follow_content_revision: 1,
          entries: copiedPlaylistPage.detail.entries.slice(0, index + 2),
        },
      }),
    );
    const settings = (index: number): Schema<"PluginSettings"> => ({
      credential_storage_ready: true,
      installations: [
        {
          ...playlistConnection,
          id: origin.installation_id,
          service_id: origin.service_id,
          revision: 8 + index,
          manifest: {
            ...playlistConnection.manifest,
            id: origin.provider,
            name: `Reviewed connection ${index + 1}`,
          },
        },
      ],
      services: [
        {
          id: origin.service_id,
          name: `Reviewed service ${index + 1}`,
          base_url: `https://owned-provider.invalid/collection-${index + 1}/`,
          revision: 5 + index,
        },
      ],
    });
    const paused = (index: number): Schema<"PlaylistFollow"> => ({
      playlist_id: pages[index].detail.playlist.id,
      generation: 3 + index,
      content_revision: 1,
      interval: "hourly",
      snapshot: origin.snapshot,
      last_refreshed_at: Math.floor(Date.now() / 1000),
      next_refresh_at: Math.floor(Date.now() / 1000) + 3600,
      status: "paused",
      error: "authority_changed",
    });
    const statusPath = (props: Schema<"PlaylistPageProps">) =>
      `/api/v1/playlists/${props.detail.playlist.id}/follow`;
    const downloadPath = (props: Schema<"PlaylistPageProps">) =>
      `/api/v1/playlists/${props.detail.playlist.id}/download`;
    const accountOf = (options?: RequestInit) =>
      new Headers(options?.headers).get("x-hito-account");
    const oldSettings = heldResponse();
    const oldStatus = heldResponse();
    const oldSubmission = heldResponse();
    let visible = 0;
    const reads = [0, 0, 0];
    const fetch = vi.fn((path: string, options?: RequestInit) => {
      const method = options?.method ?? "GET";
      if (method === "GET") {
        if (
          pages.some(
            (props) =>
              path === downloadPath(props) &&
              accountOf(options) === props.user.id,
          )
        )
          return Promise.resolve(
            Response.json({ error: "Not found" }, { status: 404 }),
          );
        const index = pages.findIndex(
          (props) =>
            path === statusPath(props) && accountOf(options) === props.user.id,
        );
        if (index !== -1) {
          reads[index]++;
          return index === 0 && reads[index] === 2
            ? oldStatus.response
            : Promise.resolve(Response.json(paused(index)));
        }
        if (
          path === "/api/v1/plugins" &&
          accountOf(options) === pages[visible].user.id
        )
          return visible === 0
            ? oldSettings.response
            : Promise.resolve(Response.json(settings(visible)));
      }
      if (
        path === "/api/v1/sync" &&
        method === "POST" &&
        accountOf(options) === pages[1].user.id
      )
        return oldSubmission.response;
      throw new Error("Unexpected request outside Resume ownership checks");
    });
    vi.stubGlobal("fetch", fetch);
    const statusCalls = (props: Schema<"PlaylistPageProps">) =>
      fetch.mock.calls.filter(
        ([path, options]) =>
          path === statusPath(props) && accountOf(options) === props.user.id,
      );
    const entries = () =>
      within(screen.getByRole("table", { name: "Playlist entries" }))
        .getAllByRole("row")
        .slice(1)
        .map((row) => row.textContent);
    const view = render(page(pages[0]));
    await act(async () => {});
    await act(async () => {
      fireEvent.click(
        screen.getByRole("button", { name: "Review connection" }),
      );
    });
    expect(reads[0]).toBe(2);
    const settingsSignal = fetch.mock.calls.find(
      ([path]) => path === "/api/v1/plugins",
    )?.[1]?.signal;
    const reviewSignal = statusCalls(pages[0])[1][1]?.signal;
    expect(settingsSignal?.aborted).toBe(false);
    expect(reviewSignal?.aborted).toBe(false);
    visible = 1;
    view.rerender(page(pages[1]));
    expect(settingsSignal?.aborted).toBe(true);
    expect(reviewSignal?.aborted).toBe(true);
    await act(async () => {});
    const secondEntries = entries();
    expect(secondEntries).toHaveLength(pages[1].detail.entries.length);
    await act(async () => {
      oldSettings.resolve(Response.json(settings(0)));
      oldStatus.resolve(Response.json(paused(0)));
    });
    expect(
      screen.getByRole("heading", { name: pages[1].detail.playlist.name }),
    ).toBeTruthy();
    expect(
      screen.queryByText(settings(0).installations[0].manifest.name),
    ).toBeNull();
    expect(screen.queryByRole("button", { name: "Resume updates" })).toBeNull();
    expect(
      screen.queryByRole("button", { name: "Retry resume request" }),
    ).toBeNull();
    expect(entries()).toEqual(secondEntries);
    expect(
      fetch.mock.calls.filter(([path]) => path === "/api/v1/sync"),
    ).toHaveLength(0);
    await act(async () => {
      fireEvent.click(
        screen.getByRole("button", { name: "Review connection" }),
      );
    });
    expect(
      screen.getByText(settings(1).installations[0].manifest.name),
    ).toBeTruthy();
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Resume updates" }));
    });
    const posts = fetch.mock.calls.filter(([path]) => path === "/api/v1/sync");
    expect(posts).toHaveLength(1);
    const body: Schema<"SyncBatch"> = JSON.parse(String(posts[0][1]?.body));
    expect(body.operations).toHaveLength(1);
    expect(body.operations[0].action).toEqual({
      kind: "playlist_follow_resume",
      playlist_id: pages[1].detail.playlist.id,
      expected_generation: paused(1).generation,
      installation_revision: settings(1).installations[0].revision,
      service_revision: settings(1).services[0].revision,
    } satisfies Schema<"Action">);
    const submissionSignal = posts[0][1]?.signal;
    expect(submissionSignal?.aborted).toBe(false);
    visible = 2;
    view.rerender(page(pages[2]));
    expect(submissionSignal?.aborted).toBe(true);
    await act(async () => {});
    const currentEntries = entries();
    expect(currentEntries).toHaveLength(pages[2].detail.entries.length);
    expect(statusCalls(pages[2])).toHaveLength(1);
    const requestsBeforeLateAcknowledgement = fetch.mock.calls.length;
    await act(async () => {
      oldSubmission.resolve(
        Response.json({
          acknowledged: [body.operations[0].id],
        } satisfies Schema<"SyncReceipt">),
      );
    });
    expect(fetch).toHaveBeenCalledTimes(requestsBeforeLateAcknowledgement);
    expect(statusCalls(pages[2])).toHaveLength(1);
    expect(
      screen.getByRole("heading", { name: pages[2].detail.playlist.name }),
    ).toBeTruthy();
    expect(
      screen.queryByText(settings(1).installations[0].manifest.name),
    ).toBeNull();
    expect(screen.queryByRole("button", { name: "Resume updates" })).toBeNull();
    expect(
      screen.queryByRole("button", { name: "Retry resume request" }),
    ).toBeNull();
    expect(
      (
        screen.getByRole("button", {
          name: "Review connection",
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(false);
    expect(screen.queryByRole("alert")).toBeNull();
    for (const props of pages) {
      expect(
        fetch.mock.calls.filter(
          ([path, options]) =>
            path === downloadPath(props) &&
            accountOf(options) === props.user.id,
        ),
      ).toEqual([
        [
          downloadPath(props),
          expect.objectContaining({
            method: "GET",
            credentials: "same-origin",
            headers: expect.objectContaining({
              "x-hito-account": props.user.id,
            }),
            signal: expect.any(AbortSignal),
          }),
        ],
      ]);
    }
    expect(entries()).toEqual(currentEntries);
    expect(router.reload).not.toHaveBeenCalled();
    expect(router.visit).not.toHaveBeenCalled();
    expect(play).not.toHaveBeenCalled();
    const beforeUnmount = fetch.mock.calls.length;
    view.unmount();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(60_000);
    });
    expect(fetch).toHaveBeenCalledTimes(beforeUnmount);
  },
);
