import { createElement, type ReactNode } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { router } from "@inertiajs/react";
import type {
  ImportedPlaylistMetadataRefresh,
  PlaylistPageProps,
  RefreshImportedPlaylistMetadata,
} from "./shared/types";
import {
  metadataDownloadPath,
  metadataOrigin,
  metadataPage,
  metadataPath,
  metadataProps,
  metadataReloadVisit,
  metadataSettings,
} from "./fixtures/playlistMetadata";

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn(), cancelAll: vi.fn() },
  Link: ({ children, href }: { children: ReactNode; href: string }) =>
    createElement("a", { href }, children),
}));

afterEach(() => {
  cleanup();
  vi.resetAllMocks();
});

test.each(["account", "playlist", "canonical origin", "unmount"] as const)(
  "an accepted album lookup cancels its own detail reload after a change of %s",
  async (change) => {
    vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
    vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
    const next: PlaylistPageProps = {
      ...metadataProps,
      ...(change === "account"
        ? { user: { ...metadataProps.user, id: "bob", username: "Bob" } }
        : {}),
      detail: {
        ...metadataProps.detail,
        playlist: {
          ...metadataProps.detail.playlist,
          ...(change === "playlist"
            ? { id: "d08be6bb-ddd5-4d03-b776-54e4f168fb46" }
            : {}),
        },
        origin: {
          ...metadataOrigin,
          ...(change === "canonical origin"
            ? { provider_playlist_id: "canonical/other-metadata-owner" }
            : {}),
        },
      },
    };
    const nextDownloadPath = `/api/v1/playlists/${next.detail.playlist.id}/download`;
    const cancelReload = vi.fn();
    vi.mocked(router.reload).mockImplementation((options) => {
      options?.onCancelToken?.({ cancel: cancelReload });
    });
    const fetch = vi.fn((path: string, options?: RequestInit) => {
      const method = options?.method ?? "GET";
      if (
        [metadataDownloadPath, nextDownloadPath].includes(path) &&
        method === "GET"
      )
        return Promise.resolve(
          Response.json({ error: "Not found" }, { status: 404 }),
        );
      if (path === "/api/v1/plugins" && method === "GET")
        return Promise.resolve(Response.json(metadataSettings));
      if (path === metadataPath && method === "POST") {
        const body: RefreshImportedPlaylistMetadata = JSON.parse(
          String(options?.body),
        );
        return Promise.resolve(
          Response.json({
            request_id: body.request_id,
            playlist_id: metadataProps.detail.playlist.id,
            updated_recordings: 1,
            unchanged_recordings: 1,
            unavailable_recordings: 0,
          } satisfies ImportedPlaylistMetadataRefresh),
        );
      }
      throw new Error("Unexpected request outside metadata detail ownership");
    });
    vi.stubGlobal("fetch", fetch);
    const view = render(metadataPage());
    const metadata = within(
      screen.getByRole("region", { name: "Playlist metadata" }),
    );
    fireEvent.click(
      metadata.getByRole("button", { name: "Find missing albums" }),
    );
    await waitFor(() => expect(router.reload).toHaveBeenCalledTimes(1));
    expect(metadata.getByRole("status").textContent).toMatch(
      /1 recording updated/,
    );
    const reload = vi.mocked(router.reload).mock.calls[0][0];
    if (!reload)
      throw new Error("The accepted lookup must request detail options");
    expect(reload).toMatchObject({
      only: ["detail", "playlists"],
    });
    expect(reload.onBefore?.(metadataReloadVisit(reload))).toBe(true);
    expect(cancelReload).not.toHaveBeenCalled();
    if (change === "unmount") view.unmount();
    else view.rerender(metadataPage(next));
    expect(cancelReload).toHaveBeenCalledTimes(1);
    expect(reload.onBefore?.(metadataReloadVisit(reload))).toBe(false);
    await act(async () => {
      reload.onNetworkError?.(
        new Error("An obsolete detail request lost its connection"),
      );
      reload.onFinish?.(metadataReloadVisit(reload));
    });
    expect(router.reload).toHaveBeenCalledTimes(1);
    expect(router.cancelAll).not.toHaveBeenCalled();
    expect(router.visit).not.toHaveBeenCalled();
    expect(
      fetch.mock.calls.filter(([path]) => path === metadataPath),
    ).toHaveLength(1);
    expect(
      fetch.mock.calls.filter(([path]) => path === "/api/v1/plugins"),
    ).toHaveLength(1);
    if (change !== "unmount") {
      const current = within(
        screen.getByRole("region", { name: "Playlist metadata" }),
      );
      expect(current.queryByRole("status")).toBeNull();
      expect(current.queryByRole("alert")).toBeNull();
      expect(
        (
          current.getByRole("button", {
            name: "Find missing albums",
          }) as HTMLButtonElement
        ).disabled,
      ).toBe(false);
    }
  },
);
