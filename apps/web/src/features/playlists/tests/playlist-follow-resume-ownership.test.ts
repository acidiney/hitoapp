import type { Schema } from "../../../shared/schema";
import { createElement, type ReactNode } from "react";
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { router } from "@inertiajs/react";
import PlaylistPage from "../pages/PlaylistPage";
import { NoticeProvider } from "../../../shared/ui/Notices";
import { PlaybackProvider } from "../../playback/components/PlaybackProvider";
import { followedPage, pausedFollow } from "../fixtures/pausedFollow";
import { copiedPlaylistPage } from "../fixtures/playlistPages";
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
  "%s changes cancel a Resume submission without applying its late response to the new owner",
  async (change) => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-10-17T03:00:00Z"));
    vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
    vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
    const play = vi
      .spyOn(HTMLMediaElement.prototype, "play")
      .mockResolvedValue();
    const accounts = ["alice", "bob"];
    const playlists = [
      copiedPlaylistPage.detail.playlist.id,
      "88fefb77-e103-4c73-8e6b-bdf6ee867184",
    ];
    const pages = accounts.map((account, index) =>
      followedPage({
        user:
          change === "account"
            ? { ...copiedPlaylistPage.user, id: account, username: account }
            : copiedPlaylistPage.user,
        detail: {
          ...followedPage().detail,
          playlist: {
            ...followedPage().detail.playlist,
            id: change === "playlist" ? playlists[index] : playlists[0],
            name: `Owned selection ${index + 1}`,
          },
          entries: copiedPlaylistPage.detail.entries.slice(0, index + 2),
        },
      }),
    );
    const statusPath = (props: Schema<"PlaylistPageProps">) =>
      `/api/v1/playlists/${props.detail.playlist.id}/follow`;
    const downloadPath = (props: Schema<"PlaylistPageProps">) =>
      `/api/v1/playlists/${props.detail.playlist.id}/download`;
    const accountOf = (options?: RequestInit) =>
      new Headers(options?.headers).get("x-hito-account");
    const oldSubmission = heldResponse();
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
        if (index !== -1)
          return Promise.resolve(
            Response.json(
              pausedFollow(pages[index].detail.playlist.id, 3 + index),
            ),
          );
      }
      if (
        path === "/api/v1/sync" &&
        method === "POST" &&
        accountOf(options) === pages[0].user.id
      )
        return oldSubmission.response;
      throw new Error("Unexpected request outside Resume ownership checks");
    });
    vi.stubGlobal("fetch", fetch);
    const entries = () =>
      within(screen.getByRole("table", { name: "Playlist entries" }))
        .getAllByRole("row")
        .slice(1)
        .map((row) => row.textContent);
    const view = render(page(pages[0]));
    await act(async () => {});
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Resume updates" }));
    });
    const posts = fetch.mock.calls.filter(([path]) => path === "/api/v1/sync");
    expect(posts).toHaveLength(1);
    const body: Schema<"SyncBatch"> = JSON.parse(String(posts[0][1]?.body));
    const submissionSignal = posts[0][1]?.signal;
    expect(submissionSignal?.aborted).toBe(false);
    view.rerender(page(pages[1]));
    expect(submissionSignal?.aborted).toBe(true);
    await act(async () => {});
    const currentEntries = entries();
    expect(currentEntries).toHaveLength(pages[1].detail.entries.length);
    const requestsBeforeLateAcknowledgement = fetch.mock.calls.length;
    await act(async () => {
      oldSubmission.resolve(
        Response.json({
          acknowledged: [body.operations[0].id],
        } satisfies Schema<"SyncReceipt">),
      );
    });
    expect(fetch).toHaveBeenCalledTimes(requestsBeforeLateAcknowledgement);
    expect(
      screen.getByRole("heading", { name: pages[1].detail.playlist.name }),
    ).toBeTruthy();
    expect(
      screen.queryByRole("button", { name: "Retry resume request" }),
    ).toBeNull();
    expect(
      (
        screen.getByRole("button", {
          name: "Resume updates",
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(false);
    expect(screen.queryByRole("alert")).toBeNull();
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
