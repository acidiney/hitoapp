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
import PlaylistPage from "./pages/PlaylistPage";
import { NoticeProvider } from "./shared/ui/Notices";
import { PlaybackProvider, usePlayer } from "./player/PlaybackProvider";
import type { Player } from "./player/model";
import type {
  ImportedPlaylistMetadataRefresh,
  RefreshImportedPlaylistMetadata,
} from "./shared/types";
import { recordingA } from "./fixtures/playlistPages";
import {
  metadataDownloadPath,
  metadataPath,
  metadataProps,
  metadataReloadPage,
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

test.each(["request", "network"] as const)(
  "confirmed album information survives a detail reload %s failure and retries only the view",
  async (failure) => {
    vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
    vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
    const play = vi
      .spyOn(HTMLMediaElement.prototype, "play")
      .mockResolvedValue();
    const logs = (["debug", "info", "warn", "error"] as const).map((level) =>
      vi.spyOn(console, level).mockImplementation(() => {}),
    );
    const rawError =
      "Private detail failure at https://provider.invalid/?signed=detail-secret";
    vi.mocked(router.reload).mockImplementation((options) => {
      if (
        failure === "request" &&
        vi.mocked(router.reload).mock.calls.length === 1
      )
        throw new Error(rawError);
      options?.onCancelToken?.({ cancel: vi.fn() });
    });
    const sourcePath = `/api/v1/recordings/${recordingA}/playback-sources`;
    const fetch = vi.fn((path: string, options?: RequestInit) => {
      const method = options?.method ?? "GET";
      if (path === metadataDownloadPath && method === "GET")
        return Promise.resolve(
          Response.json({ error: "Not found" }, { status: 404 }),
        );
      if (path === sourcePath && method === "GET")
        return Promise.resolve(
          Response.json([
            {
              id: "fixture-source-a",
              recording_id: recordingA,
              provider: "fixture",
              device_id: "server",
              ready: true,
            },
          ]),
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
      throw new Error("Unexpected request outside metadata detail recovery");
    });
    vi.stubGlobal("fetch", fetch);
    let player!: Player;
    function Page() {
      player = usePlayer();
      return createElement(PlaylistPage, metadataProps);
    }
    render(
      createElement(
        NoticeProvider,
        null,
        createElement(PlaybackProvider, {
          account: metadataProps.user.id,
          children: createElement(Page),
        }),
      ),
    );
    const table = screen.getByRole("table", { name: "Playlist entries" });
    const entries = within(table).getAllByRole("row").slice(1);
    const originalEntries = entries.map((row) => row.textContent);
    fireEvent.click(
      within(entries[3]).getByRole("button", { name: "Play Fixture A" }),
    );
    await waitFor(() => expect(play).toHaveBeenCalledTimes(1));
    const originalPlayer = player;
    const selection = player.queueSnapshot();
    const queue = await player.queuePage(0, 10);
    const metadata = within(
      screen.getByRole("region", { name: "Playlist metadata" }),
    );
    fireEvent.click(
      metadata.getByRole("button", { name: "Find missing albums" }),
    );
    await waitFor(() => expect(router.reload).toHaveBeenCalledTimes(1));
    const first = vi.mocked(router.reload).mock.calls[0][0];
    if (!first)
      throw new Error("The accepted receipt must trigger a detail reload");
    if (failure === "network") {
      expect(metadata.getByRole("status").textContent).toMatch(
        /1 recording updated.*1 unchanged/,
      );
      await act(async () => {
        first.onNetworkError?.(new Error(rawError));
        first.onFinish?.(metadataReloadVisit(first));
      });
    }
    expect(metadata.getByRole("status").textContent).toMatch(
      /1 recording updated.*1 unchanged/,
    );
    expect(metadata.getByRole("alert").textContent).toMatch(
      /album information was saved.*details could not be loaded/i,
    );
    expect(
      metadata.queryByRole("button", { name: "Retry album lookup" }),
    ).toBeNull();
    const retry = metadata.getByRole("button", {
      name: "Reload playlist details",
    });
    const callsBeforeRetry = fetch.mock.calls.length;
    await act(async () => {
      fireEvent.click(retry);
      fireEvent.click(retry);
    });
    expect(router.reload).toHaveBeenCalledTimes(2);
    expect((retry as HTMLButtonElement).disabled).toBe(true);
    expect(fetch).toHaveBeenCalledTimes(callsBeforeRetry);
    const second = vi.mocked(router.reload).mock.calls[1][0];
    if (!second) throw new Error("The view retry needs partial reload options");
    expect(second).toMatchObject({ only: ["detail", "playlists"] });
    await act(async () => {
      await second.onSuccess?.(metadataReloadPage());
      second.onFinish?.(metadataReloadVisit(second, true));
    });
    expect(metadata.queryByRole("alert")).toBeNull();
    expect(
      metadata.queryByRole("button", { name: "Reload playlist details" }),
    ).toBeNull();
    expect(metadata.getByRole("status").textContent).toMatch(
      /1 recording updated.*1 unchanged/,
    );
    expect(
      screen.getByRole("heading", { name: metadataProps.detail.playlist.name }),
    ).toBeTruthy();
    expect(
      within(table)
        .getAllByRole("row")
        .slice(1)
        .map((row) => row.textContent),
    ).toEqual(originalEntries);
    expect(player).toBe(originalPlayer);
    expect(player.queueSnapshot()).toEqual(selection);
    expect(await player.queuePage(0, 10)).toEqual(queue);
    expect(player.snapshot().current?.id).toBe(recordingA);
    expect(play).toHaveBeenCalledTimes(1);
    expect(router.visit).not.toHaveBeenCalled();
    expect(router.cancelAll).not.toHaveBeenCalled();
    expect(fetch.mock.calls.map(([path]) => path)).toEqual([
      metadataDownloadPath,
      sourcePath,
      "/api/v1/plugins",
      metadataPath,
    ]);
    expect(document.body.textContent).not.toContain(rawError);
    expect(
      JSON.stringify(logs.flatMap((sink) => sink.mock.calls)),
    ).not.toContain(rawError);
  },
);
