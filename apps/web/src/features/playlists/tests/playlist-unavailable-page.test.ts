import type { Schema } from "../../../shared/schema";
import { createElement, type ReactNode } from "react";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { router } from "@inertiajs/react";
import PlaylistPage from "../pages/PlaylistPage";
import { NoticeProvider } from "../../../shared/ui/Notices";
import {
  PlaybackProvider,
  usePlayer,
} from "../../playback/components/PlaybackProvider";
import type { Player } from "../../playback/model";

import { unavailablePlaylistPages } from "../fixtures/playlistPages";

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({
    children,
    href,
    ...attributes
  }: {
    children: ReactNode;
    href: string;
  }) => createElement("a", { href, ...attributes }, children),
}));

test("retained playlists remain editable and known recordings resolve audio only after explicit Play", async () => {
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  for (const props of unavailablePlaylistPages) {
    vi.mocked(router.reload).mockClear();
    const downloadPath = `/api/v1/playlists/${props.detail.playlist.id}/download`;
    const recordings = props.detail.entries.flatMap((entry) =>
      entry.recording_id == null ? [] : [entry.recording_id],
    );
    const fetch = vi
      .fn()
      .mockImplementation(async (path: string, options: RequestInit) => {
        expect(new Headers(options.headers).get("x-hito-account")).toBe(
          props.user.id,
        );
        if (path === downloadPath && options.method === "GET")
          return Response.json({ error: "Not found" }, { status: 404 });
        const recording = recordings.find(
          (id) =>
            path === `/api/v1/recordings/${id}/playback-sources` ||
            path.startsWith(`/api/v1/recordings/${id}/sources`),
        );
        if (recording) {
          const sourcePath = `/api/v1/recordings/${recording}/sources`;
          if (
            path === `/api/v1/recordings/${recording}/playback-sources` &&
            options.method === "GET"
          )
            return Response.json([]);
          if (path === `${sourcePath}/refresh` && options.method === "POST") {
            expect(JSON.parse(String(options.body))).toEqual({
              purpose: "playback",
            });
            return Response.json([]);
          }
          if (path === `${sourcePath}/resolve` && options.method === "POST") {
            const body: Schema<"ResolveSource"> = JSON.parse(
              String(options.body),
            );
            expect(Object.keys(body)).toEqual(["request_id"]);
            expect(body.request_id).toMatch(
              /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i,
            );
            return Response.json({
              request_id: body.request_id,
              resolution_id: crypto.randomUUID(),
              recording_id: recording,
              outcome: "no_suitable_source",
              sources: [],
            } satisfies Schema<"SourceResolution">);
          }
        }
        if (path !== "/api/v1/sync")
          throw new Error("Unexpected owned playlist request");
        const batch: Schema<"SyncBatch"> = JSON.parse(String(options.body));
        return Response.json({
          acknowledged: batch.operations.map((operation) => operation.id),
        });
      });
    vi.stubGlobal("fetch", fetch);
    let player!: Player;
    function Page() {
      player = usePlayer();
      return createElement(PlaylistPage, props);
    }
    const view = render(
      createElement(
        NoticeProvider,
        null,
        createElement(PlaybackProvider, {
          account: props.user.id,
          children: createElement(Page),
        }),
      ),
    );
    const table = screen.getByRole("table", { name: "Playlist entries" });
    const rows = within(table).getAllByRole("row").slice(1);
    expect(rows).toHaveLength(2);
    expect(screen.getByText(/2 entries/)).toBeTruthy();
    expect(screen.queryByText("This playlist is ready for music")).toBeNull();
    expect(screen.getByText(/Curated by the fixture owner/)).toBeTruthy();
    const toolbar = screen.getByRole("button", { name: "Play playlist" });
    expect(toolbar.hasAttribute("disabled")).toBe(recordings.length === 0);
    for (const [index, entry] of props.detail.entries.entries()) {
      const row = within(rows[index]);
      expect(row.getByText(entry.title)).toBeTruthy();
      if (entry.recording_id == null) {
        expect(row.queryAllByRole("link")).toHaveLength(0);
        expect(row.queryAllByRole("button")).toHaveLength(0);
        expect(
          row.queryByRole("button", { name: /Play|Like|Unlike|Details/ }),
        ).toBeNull();
        expect(rows[index].querySelector("img")).toBeNull();
        expect(row.getAllByRole("cell")[4].textContent?.trim()).toBe("—");
        expect(
          row.getByText(
            entry.origin?.kind === "unknown"
              ? "Unknown item from its provider"
              : "Unavailable from its provider",
          ),
        ).toBeTruthy();
      } else {
        const rowPlay = row.getByRole("button", {
          name: `Play ${entry.title}`,
        });
        const titlePlay = row.getByRole("button", { name: entry.title });
        expect(rowPlay.hasAttribute("disabled")).toBe(false);
        expect(titlePlay.hasAttribute("disabled")).toBe(false);
        expect(row.getByText("Plays from your services")).toBeTruthy();
        expect(
          row
            .getByRole("link", { name: `Details for ${entry.title}` })
            .getAttribute("href"),
        ).toBe(`/recordings/${entry.recording_id}`);
        expect(
          row
            .getByRole("button", { name: `Like ${entry.title}` })
            .hasAttribute("disabled"),
        ).toBe(false);
      }
    }
    expect(player.queueSnapshot().total).toBe(0);
    expect((await player.queuePage(0, 10)).entries).toEqual([]);
    expect(fetch).toHaveBeenCalledTimes(1);
    expect(fetch.mock.calls[0]).toEqual([
      downloadPath,
      expect.objectContaining({
        method: "GET",
        credentials: "same-origin",
        headers: expect.objectContaining({ "x-hito-account": props.user.id }),
        signal: expect.any(AbortSignal),
      }),
    ]);
    expect(
      fetch.mock.calls.filter(([, options]) => options?.method !== "GET"),
    ).toHaveLength(0);
    expect(play).not.toHaveBeenCalled();
    fireEvent.click(toolbar);
    if (recordings.length === 0) {
      expect(fetch).toHaveBeenCalledTimes(1);
      expect(player.queueSnapshot().total).toBe(0);
      expect((await player.queuePage(0, 10)).entries).toEqual([]);
    } else {
      await waitFor(() => {
        expect(player.snapshot().loading).toBe(false);
        expect(player.snapshot().error).toContain("Audio is unavailable");
        expect(fetch).toHaveBeenCalledTimes(1 + recordings.length * 3);
      });
      const sourceCalls = fetch.mock.calls.slice(1);
      expect(
        sourceCalls.map(([path, options]) => [path, options.method]),
      ).toEqual(
        recordings.flatMap((recording) => [
          [`/api/v1/recordings/${recording}/playback-sources`, "GET"],
          [`/api/v1/recordings/${recording}/sources/refresh`, "POST"],
          [`/api/v1/recordings/${recording}/sources/resolve`, "POST"],
        ]),
      );
      for (const [, options] of sourceCalls)
        expect(options).toEqual(
          expect.objectContaining({
            credentials: "same-origin",
            headers: expect.objectContaining({
              "x-hito-account": props.user.id,
            }),
            signal: expect.any(AbortSignal),
          }),
        );
      const requestIds = sourceCalls
        .filter(([path]) => path.endsWith("/resolve"))
        .map(([, options]) => JSON.parse(String(options.body)).request_id);
      expect(new Set(requestIds).size).toBe(recordings.length);
      const queue = await player.queuePage(0, 10);
      expect(queue.entries.map((entry) => entry.recording.id)).toEqual(
        recordings,
      );
      expect(new Set(queue.entries.map((entry) => entry.id)).size).toBe(
        recordings.length,
      );
      expect(player.snapshot().playing).toBe(false);
      expect(play).not.toHaveBeenCalled();
      act(() => player.clear());
    }
    const beforeRemoval = fetch.mock.calls.length;
    const removed = props.detail.entries[1];
    fireEvent.click(screen.getByRole("button", { name: "Edit" }));
    fireEvent.click(
      within(rows[1]).getByRole("button", {
        name: `Remove ${removed.title} from playlist`,
      }),
    );
    await waitFor(() => expect(router.reload).toHaveBeenCalledTimes(1));
    expect(fetch).toHaveBeenCalledTimes(beforeRemoval + 1);
    expect(fetch.mock.calls[beforeRemoval][0]).toBe("/api/v1/sync");
    expect(
      JSON.parse(fetch.mock.calls[beforeRemoval][1].body).operations,
    ).toEqual([
      {
        id: expect.any(String),
        action: {
          kind: "entry_remove",
          playlist_id: props.detail.playlist.id,
          entry_id: removed.entry_id,
        },
      },
    ]);
    expect(player.queueSnapshot().total).toBe(0);
    view.unmount();
  }
});
