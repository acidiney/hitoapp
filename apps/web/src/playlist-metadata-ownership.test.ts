import { createElement, type ReactNode } from "react";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { router } from "@inertiajs/react";
import type {
  ImportedPlaylistMetadataRefresh,
  PlaylistPageProps,
  RefreshImportedPlaylistMetadata,
} from "./shared/types";
import {
  heldMetadataResponse,
  metadataDownloadPath,
  metadataOrigin,
  metadataPage,
  metadataPath,
  metadataProps,
  metadataSettings,
} from "./fixtures/playlistMetadata";

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ children, href }: { children: ReactNode; href: string }) =>
    createElement("a", { href }, children),
}));

test.each(["account", "playlist", "canonical origin", "unmount"] as const)(
  "a late album lookup receipt cannot cross a change of %s",
  async (change) => {
    vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
    vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
    const play = vi
      .spyOn(HTMLMediaElement.prototype, "play")
      .mockResolvedValue();
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
            ? { id: "b37464a4-951d-4e64-a21c-210b54cf33aa" }
            : {}),
        },
        origin: {
          ...metadataOrigin,
          ...(change === "canonical origin"
            ? { provider_playlist_id: "canonical/a-different-playlist" }
            : {}),
        },
      },
    };
    const nextDownloadPath = `/api/v1/playlists/${next.detail.playlist.id}/download`;
    const held = heldMetadataResponse();
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
      if (path === metadataPath && method === "POST") return held.response;
      throw new Error("Unexpected request outside owned metadata lookup");
    });
    vi.stubGlobal("fetch", fetch);
    const view = render(metadataPage());
    const metadata = within(
      screen.getByRole("region", { name: "Playlist metadata" }),
    );
    fireEvent.click(
      metadata.getByRole("button", { name: "Find missing albums" }),
    );
    const posts = () =>
      fetch.mock.calls.filter(([path]) => path === metadataPath);
    await waitFor(() => expect(posts()).toHaveLength(1));
    const oldOptions = posts()[0][1];
    const body: RefreshImportedPlaylistMetadata = JSON.parse(
      String(oldOptions?.body),
    );
    expect(oldOptions?.signal?.aborted).toBe(false);
    expect(router.reload).not.toHaveBeenCalled();
    if (change === "unmount") view.unmount();
    else view.rerender(metadataPage(next));
    expect(oldOptions?.signal?.aborted).toBe(true);
    await act(async () =>
      held.resolve(
        Response.json({
          request_id: body.request_id,
          playlist_id: metadataProps.detail.playlist.id,
          updated_recordings: 1,
          unchanged_recordings: 1,
          unavailable_recordings: 0,
        } satisfies ImportedPlaylistMetadataRefresh),
      ),
    );
    expect(router.reload).not.toHaveBeenCalled();
    expect(router.visit).not.toHaveBeenCalled();
    expect(play).not.toHaveBeenCalled();
    expect(posts()).toHaveLength(1);
    expect(
      fetch.mock.calls.filter(([path]) => path === "/api/v1/plugins"),
    ).toHaveLength(1);
    for (const [path, options] of fetch.mock.calls)
      if (path !== metadataPath) expect(options?.method).toBe("GET");
    if (change !== "unmount") {
      const nextMetadata = within(
        screen.getByRole("region", { name: "Playlist metadata" }),
      );
      expect(nextMetadata.queryByRole("status")).toBeNull();
      expect(nextMetadata.queryByRole("alert")).toBeNull();
      expect(
        nextMetadata.queryByRole("button", { name: "Retry album lookup" }),
      ).toBeNull();
      expect(
        (
          nextMetadata.getByRole("button", {
            name: "Find missing albums",
          }) as HTMLButtonElement
        ).disabled,
      ).toBe(false);
      expect(
        screen.getByRole("heading", { name: next.detail.playlist.name }),
      ).toBeTruthy();
      expect(
        within(
          screen.getByRole("table", { name: "Playlist entries" }),
        ).getAllByRole("row"),
      ).toHaveLength(next.detail.entries.length + 1);
    }
  },
);
