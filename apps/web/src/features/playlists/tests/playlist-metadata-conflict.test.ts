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

import {
  heldMetadataResponse,
  metadataDownloadPath,
  metadataInstallation,
  metadataPage,
  metadataPath,
  metadataProps,
  metadataSettings,
} from "../fixtures/playlistMetadata";

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ children, href }: { children: ReactNode; href: string }) =>
    createElement("a", { href }, children),
}));

test("a rejected album lookup requires an explicit new lookup against the current connection", async () => {
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const logs = (["debug", "info", "warn", "error"] as const).map((level) =>
    vi.spyOn(console, level).mockImplementation(() => {}),
  );
  const currentRevision = metadataInstallation.revision + 1;
  const updatedSettings = {
    ...metadataSettings,
    installations: [{ ...metadataInstallation, revision: currentRevision }],
  };
  const rawError =
    "The connection changed at https://private-provider.invalid/?token=secret-conflict";
  const requests: Schema<"RefreshImportedPlaylistMetadata">[] = [];
  let settingsReads = 0;
  const acceptedResponse = heldMetadataResponse();
  const fetch = vi.fn((path: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    if (path === metadataDownloadPath && method === "GET")
      return Promise.resolve(
        Response.json({ error: "Not found" }, { status: 404 }),
      );
    if (path === "/api/v1/plugins" && method === "GET") {
      settingsReads++;
      return Promise.resolve(
        Response.json(settingsReads === 1 ? metadataSettings : updatedSettings),
      );
    }
    if (path === metadataPath && method === "POST") {
      requests.push(JSON.parse(String(options?.body)));
      if (requests.length === 1)
        return Promise.resolve(
          Response.json(
            { error: rawError, code: "operation_rejected" },
            { status: 409 },
          ),
        );
      if (requests.length === 2) return acceptedResponse.response;
    }
    throw new Error(
      "Unexpected request outside explicit metadata conflict recovery",
    );
  });
  vi.stubGlobal("fetch", fetch);
  render(metadataPage());
  const metadata = within(
    screen.getByRole("region", { name: "Playlist metadata" }),
  );
  const lookup = metadata.getByRole("button", { name: "Find missing albums" });
  const table = screen.getByRole("table", { name: "Playlist entries" });
  const originalEntries = within(table)
    .getAllByRole("row")
    .map((row) => row.textContent);
  fireEvent.click(lookup);
  await metadata.findByRole("alert");
  expect(
    metadata.queryByRole("button", { name: "Retry album lookup" }),
  ).toBeNull();
  expect(metadata.getByRole("alert").textContent).toMatch(
    /playlist or its connection changed/i,
  );
  expect(metadata.queryByRole("status")).toBeNull();
  expect((lookup as HTMLButtonElement).disabled).toBe(false);
  expect(requests).toHaveLength(1);
  expect(requests[0]).toEqual({
    request_id: expect.any(String),
    installation_revision: metadataInstallation.revision,
  });
  expect(settingsReads).toBe(1);
  expect(router.reload).not.toHaveBeenCalled();
  await act(async () => {});
  expect(requests).toHaveLength(1);
  expect(settingsReads).toBe(1);
  await act(async () => {
    fireEvent.click(lookup);
    fireEvent.click(lookup);
  });
  await waitFor(() => expect(requests).toHaveLength(2));
  expect(settingsReads).toBe(2);
  expect(requests[1]).toEqual({
    request_id: expect.any(String),
    installation_revision: currentRevision,
  });
  expect(requests[1].request_id).not.toBe(requests[0].request_id);
  expect((lookup as HTMLButtonElement).disabled).toBe(true);
  expect(router.reload).not.toHaveBeenCalled();
  await act(async () =>
    acceptedResponse.resolve(
      Response.json({
        request_id: requests[1].request_id,
        playlist_id: metadataProps.detail.playlist.id,
        updated_recordings: 1,
        unchanged_recordings: 1,
        unavailable_recordings: 0,
      } satisfies Schema<"ImportedPlaylistMetadataRefresh">),
    ),
  );
  expect(metadata.queryByRole("alert")).toBeNull();
  expect(metadata.getByRole("status").textContent).toMatch(
    /1 recording updated.*1 unchanged/,
  );
  expect(router.reload).toHaveBeenCalledExactlyOnceWith(
    expect.objectContaining({ only: ["detail", "playlists"] }),
  );
  expect(router.visit).not.toHaveBeenCalled();
  expect(
    within(table)
      .getAllByRole("row")
      .map((row) => row.textContent),
  ).toEqual(originalEntries);
  expect(play).not.toHaveBeenCalled();
  expect(fetch.mock.calls.map(([path]) => path)).toEqual([
    metadataDownloadPath,
    "/api/v1/plugins",
    metadataPath,
    "/api/v1/plugins",
    metadataPath,
  ]);
  expect(document.body.textContent).not.toContain(rawError);
  expect(JSON.stringify(logs.flatMap((sink) => sink.mock.calls))).not.toContain(
    rawError,
  );
});
