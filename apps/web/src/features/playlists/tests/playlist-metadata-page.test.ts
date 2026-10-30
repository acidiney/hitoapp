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
import type { Schema } from "../../../shared/schema";
import {
  copiedPlaylistPage as props,
  recordingA,
} from "../fixtures/playlistPages";
import { playlistConnection } from "../fixtures/playlistImports";

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ children, href }: { children: ReactNode; href: string }) =>
    createElement("a", { href }, children),
}));

test("an imported copy finds missing albums only on request without changing its entries or current playback", async () => {
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const origin = props.detail.origin;
  if (!origin)
    throw new Error("The copied playlist needs its original provider");
  const installation: Schema<"PluginInstallation"> = {
    ...playlistConnection,
    id: origin.installation_id,
    service_id: origin.service_id,
    revision: origin.installation_revision + 4,
    manifest: { ...playlistConnection.manifest, id: origin.provider },
  };
  const settings: Schema<"PluginSettings"> = {
    credential_storage_ready: true,
    installations: [
      { ...installation, id: "unrelated-installation", revision: 99 },
      installation,
    ],
    services: [
      {
        id: origin.service_id,
        name: "Playlist fixture service",
        base_url: "https://provider.invalid/",
        revision: origin.service_revision + 1,
      },
    ],
  };
  const downloadPath = `/api/v1/playlists/${props.detail.playlist.id}/download`;
  const sourcePath = `/api/v1/recordings/${recordingA}/playback-sources`;
  const metadataPath = `/api/v1/playlists/${props.detail.playlist.id}/metadata/refresh`;
  let resolveMetadata!: (response: Response) => void;
  const metadataResponse = new Promise<Response>((resolve) => {
    resolveMetadata = resolve;
  });
  const fetch = vi.fn((path: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    if (path === downloadPath && method === "GET")
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
      return Promise.resolve(Response.json(settings));
    if (path === metadataPath && method === "POST") return metadataResponse;
    throw new Error(
      "Unexpected request outside explicit playlist metadata repair",
    );
  });
  vi.stubGlobal("fetch", fetch);
  let player!: Player;
  function Page() {
    player = usePlayer();
    return createElement(PlaylistPage, props);
  }
  render(
    createElement(
      NoticeProvider,
      null,
      createElement(PlaybackProvider, {
        account: props.user.id,
        children: createElement(Page),
      }),
    ),
  );
  await act(async () => {});
  const metadata = within(
    screen.getByRole("region", { name: "Playlist metadata" }),
  );
  const lookup = metadata.getByRole("button", { name: "Find missing albums" });
  expect(fetch.mock.calls.map(([path]) => path)).toEqual([downloadPath]);
  const table = screen.getByRole("table", { name: "Playlist entries" });
  const entries = within(table).getAllByRole("row").slice(1);
  const originalEntries = entries.map((row) => row.textContent);
  fireEvent.click(
    within(entries[3]).getByRole("button", { name: "Play Fixture A" }),
  );
  await waitFor(() => expect(player.snapshot().current?.id).toBe(recordingA));
  await waitFor(() => expect(play).toHaveBeenCalledTimes(1));
  const originalPlayer = player;
  const originalQueue = await player.queuePage(0, 10);
  const originalSelection = player.queueSnapshot();
  await act(async () => {
    fireEvent.click(lookup);
    fireEvent.click(lookup);
  });
  await waitFor(() =>
    expect(fetch.mock.calls.map(([path]) => path)).toEqual([
      downloadPath,
      sourcePath,
      "/api/v1/plugins",
      metadataPath,
    ]),
  );
  const requestOptions = fetch.mock.calls[3][1];
  expect(requestOptions).toMatchObject({
    method: "POST",
    credentials: "same-origin",
    headers: { "x-hito-account": props.user.id },
    signal: expect.any(AbortSignal),
  });
  const request: Schema<"RefreshImportedPlaylistMetadata"> = JSON.parse(
    String(requestOptions?.body),
  );
  expect(request).toEqual({
    request_id: expect.stringMatching(
      /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
    ),
    installation_revision: installation.revision,
  });
  expect((lookup as HTMLButtonElement).disabled).toBe(true);
  expect(router.reload).not.toHaveBeenCalled();
  await act(async () =>
    resolveMetadata(
      Response.json({
        request_id: request.request_id,
        playlist_id: props.detail.playlist.id,
        updated_recordings: 1,
        unchanged_recordings: 0,
        unavailable_recordings: 1,
      } satisfies Schema<"ImportedPlaylistMetadataRefresh">),
    ),
  );
  await waitFor(() =>
    expect(router.reload).toHaveBeenCalledExactlyOnceWith(
      expect.objectContaining({
        only: ["detail", "playlists"],
      }),
    ),
  );
  expect(metadata.getByRole("status").textContent).toMatch(
    /1 recording updated.*0 unchanged.*1 unavailable/i,
  );
  expect(
    screen.getByRole("heading", { name: props.detail.playlist.name }),
  ).toBeTruthy();
  expect(
    within(table)
      .getAllByRole("row")
      .slice(1)
      .map((row) => row.textContent),
  ).toEqual(originalEntries);
  expect(player).toBe(originalPlayer);
  expect(player.queueSnapshot()).toEqual(originalSelection);
  expect(await player.queuePage(0, 10)).toEqual(originalQueue);
  expect(player.snapshot().current?.id).toBe(recordingA);
  expect(play).toHaveBeenCalledTimes(1);
  expect(router.visit).not.toHaveBeenCalled();
  expect(fetch).toHaveBeenCalledTimes(4);
});
