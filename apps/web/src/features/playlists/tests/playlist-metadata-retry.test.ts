import { createElement, type ReactNode } from "react";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { router } from "@inertiajs/react";
import PlaylistPage from "../pages/PlaylistPage";
import { NoticeProvider } from "../../../shared/ui/Notices";
import { PlaybackProvider, usePlayer } from "../../../player/PlaybackProvider";
import type { Player } from "../../../player/model";
import type { components } from "../../../generated";
import {
  copiedPlaylistPage as props,
  recordingA,
} from "../fixtures/playlistPages";
import { playlistConnection } from "../fixtures/playlistImports";

type Schema = components["schemas"];

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ children, href }: { children: ReactNode; href: string }) =>
    createElement("a", { href }, children),
}));

test("a lost album lookup response retries its frozen operation while the provider is offline", async () => {
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const logs = (["debug", "info", "warn", "error"] as const).map((level) =>
    vi.spyOn(console, level).mockImplementation(() => {}),
  );
  const origin = props.detail.origin;
  if (!origin)
    throw new Error("The copied playlist needs its original provider");
  const installation: Schema["PluginInstallation"] = {
    ...playlistConnection,
    id: origin.installation_id,
    service_id: origin.service_id,
    revision: origin.installation_revision + 6,
    manifest: { ...playlistConnection.manifest, id: origin.provider },
  };
  const settings: Schema["PluginSettings"] = {
    credential_storage_ready: true,
    installations: [installation],
    services: [
      {
        id: origin.service_id,
        name: "Private fixture service",
        base_url: "https://private-provider.invalid/?credential=secret-canary",
        revision: origin.service_revision + 2,
      },
    ],
  };
  const downloadPath = `/api/v1/playlists/${props.detail.playlist.id}/download`;
  const metadataPath = `/api/v1/playlists/${props.detail.playlist.id}/metadata/refresh`;
  const sourcePath = `/api/v1/recordings/${recordingA}/playback-sources`;
  const rawError =
    "Lost reply from https://private-provider.invalid/audio?signed=private-canary";
  let settingsReads = 0;
  let metadataPosts = 0;
  let accepted: Schema["ImportedPlaylistMetadataRefresh"] | null = null;
  let resolveRetry!: (response: Response) => void;
  const retryResponse = new Promise<Response>((resolve) => {
    resolveRetry = resolve;
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
    if (path === "/api/v1/plugins" && method === "GET") {
      settingsReads++;
      return settingsReads === 1
        ? Promise.resolve(Response.json(settings))
        : Promise.reject(
            new TypeError("The connection settings are now offline"),
          );
    }
    if (path === metadataPath && method === "POST") {
      metadataPosts++;
      if (metadataPosts === 1) {
        const body: Schema["RefreshImportedPlaylistMetadata"] = JSON.parse(
          String(options?.body),
        );
        accepted = {
          request_id: body.request_id,
          playlist_id: props.detail.playlist.id,
          updated_recordings: 1,
          unchanged_recordings: 1,
          unavailable_recordings: 0,
        };
        return Promise.reject(new TypeError(rawError));
      }
      if (metadataPosts === 2) return retryResponse;
    }
    throw new Error("Unexpected request outside album receipt recovery");
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
  const table = screen.getByRole("table", { name: "Playlist entries" });
  const entries = within(table).getAllByRole("row").slice(1);
  const originalEntries = entries.map((row) => row.textContent);
  fireEvent.click(
    within(entries[3]).getByRole("button", { name: "Play Fixture A" }),
  );
  await waitFor(() => expect(play).toHaveBeenCalledTimes(1));
  const originalQueue = await player.queuePage(0, 10);
  const selection = player.queueSnapshot();
  const metadata = within(
    screen.getByRole("region", { name: "Playlist metadata" }),
  );
  fireEvent.click(
    metadata.getByRole("button", { name: "Find missing albums" }),
  );
  await metadata.findByRole("alert");
  const retry = metadata.getByRole("button", { name: "Retry album lookup" });
  expect(metadata.getByRole("alert").textContent).toMatch(
    /could not confirm.*retry the same request/i,
  );
  expect(
    (
      metadata.getByRole("button", {
        name: "Find missing albums",
      }) as HTMLButtonElement
    ).disabled,
  ).toBe(true);
  expect(router.reload).not.toHaveBeenCalled();
  const posts = () =>
    fetch.mock.calls.filter(([path]) => path === metadataPath);
  expect(posts()).toHaveLength(1);
  const frozenBody = String(posts()[0][1]?.body);
  expect(JSON.parse(frozenBody)).toEqual({
    request_id: expect.any(String),
    installation_revision: installation.revision,
  });
  settings.installations[0] = {
    ...installation,
    revision: installation.revision + 10,
  };
  await act(async () => {
    window.dispatchEvent(new Event("focus"));
  });
  expect(settingsReads).toBe(1);
  await act(async () => {
    fireEvent.click(retry);
    fireEvent.click(retry);
  });
  expect(posts()).toHaveLength(2);
  expect((retry as HTMLButtonElement).disabled).toBe(true);
  expect(posts()[1][1]).toMatchObject({
    method: "POST",
    credentials: "same-origin",
    headers: { "x-hito-account": props.user.id },
    body: frozenBody,
    signal: expect.any(AbortSignal),
  });
  expect(posts()[1][1]?.signal).not.toBe(posts()[0][1]?.signal);
  expect(posts()[1][1]?.signal?.aborted).toBe(false);
  expect(settingsReads).toBe(1);
  expect(router.reload).not.toHaveBeenCalled();
  if (!accepted)
    throw new Error("The first request must have committed a receipt");
  await act(async () => resolveRetry(Response.json(accepted)));
  expect(metadata.getByRole("status").textContent).toMatch(
    /1 recording updated.*1 unchanged.*0 unavailable/i,
  );
  expect(
    metadata.queryByRole("button", { name: "Retry album lookup" }),
  ).toBeNull();
  expect(router.reload).toHaveBeenCalledExactlyOnceWith(
    expect.objectContaining({ only: ["detail", "playlists"] }),
  );
  expect(
    within(table)
      .getAllByRole("row")
      .slice(1)
      .map((row) => row.textContent),
  ).toEqual(originalEntries);
  expect(await player.queuePage(0, 10)).toEqual(originalQueue);
  expect(player.queueSnapshot()).toEqual(selection);
  expect(player.snapshot().current?.id).toBe(recordingA);
  expect(play).toHaveBeenCalledTimes(1);
  expect(router.visit).not.toHaveBeenCalled();
  expect(fetch.mock.calls.map(([path]) => path)).toEqual([
    downloadPath,
    sourcePath,
    "/api/v1/plugins",
    metadataPath,
    downloadPath,
    metadataPath,
  ]);
  const downloadReads = fetch.mock.calls.filter(
    ([path]) => path === downloadPath,
  );
  expect(downloadReads).toHaveLength(2);
  for (const [, options] of downloadReads) expect(options?.method).toBe("GET");
  const capturedLogs = JSON.stringify(logs.flatMap((sink) => sink.mock.calls));
  for (const privateValue of [
    rawError,
    settings.services[0].base_url,
    origin.snapshot,
    props.detail.playlist.name,
    ...props.detail.entries.map((entry) => entry.title),
  ]) {
    expect(capturedLogs).not.toContain(privateValue);
    expect(document.body.textContent).not.toContain(rawError);
  }
});
