import type { Schema } from "../../../shared/schema";
import { createElement, type ReactNode } from "react";
import { act, render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import PlaylistPage from "../../playlists/pages/PlaylistPage";
import { NoticeProvider } from "../../../shared/ui/Notices";
import { PlaybackProvider } from "../../playback/components/PlaybackProvider";
import { copiedPlaylistPage } from "../../playlists/fixtures/playlistPages";

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ href, children }: { href: string; children: ReactNode }) =>
    createElement("a", { href }, children),
}));

test("playlist entries waiting for disk space say so and are not counted as failures", async () => {
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const props = copiedPlaylistPage;
  const path = `/api/v1/playlists/${props.detail.playlist.id}/download`;
  const status: Schema<"PlaylistDownloadStatus"> = {
    retention_id: "fe8d8202-a7d2-4a17-b3c8-d471a00b5823",
    playlist_id: props.detail.playlist.id,
    host_device_id: "owned-download-host",
    desired_revision: 1,
    reconciled_revision: 1,
    entries: {
      items: props.detail.entries.map((entry) => ({
        entry_id: entry.entry_id,
        recording_id: entry.recording_id,
        status: "queued",
        source_id: null,
        error: "insufficient_storage",
      })),
      next_cursor: null,
    },
  };
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, options?: RequestInit) => {
      if (url === path && (options?.method ?? "GET") === "GET")
        return Response.json(status);
      throw new Error("A waiting playlist download must not write");
    }),
  );
  render(
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
  const strip = within(
    screen.getByRole("region", { name: "Playlist download" }),
  );
  const count = status.entries.items.length;
  expect(
    strip.getByText(
      `${count} ${count === 1 ? "entry is" : "entries are"} waiting for disk space on this server. Hito will try again.`,
    ),
  ).toBeTruthy();
  expect(strip.queryByText(/could not be downloaded/)).toBeNull();
  expect(strip.queryByRole("button", { name: /retry failed/i })).toBeNull();
});
