import { createElement, type ReactNode } from "react";
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { router } from "@inertiajs/react";
import type {
  ImportedPlaylistMetadataRefresh,
  RefreshImportedPlaylistMetadata,
} from "../../../shared/types";
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

test("a busy retry after a lost album lookup response retains the original request until its own receipt arrives", async () => {
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const bodies: string[] = [];
  let settingsReads = 0;
  const originalReceipt = heldMetadataResponse();
  const fetch = vi.fn((path: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    if (path === metadataDownloadPath && method === "GET")
      return Promise.resolve(
        Response.json({ error: "Not found" }, { status: 404 }),
      );
    if (path === "/api/v1/plugins" && method === "GET") {
      settingsReads++;
      return settingsReads === 1
        ? Promise.resolve(Response.json(metadataSettings))
        : Promise.reject(new TypeError("Connection settings are now offline"));
    }
    if (path === metadataPath && method === "POST") {
      bodies.push(String(options?.body));
      if (bodies.length === 1)
        return Promise.reject(
          new TypeError(
            "The original lookup is still running after its connection closed",
          ),
        );
      if (bodies.length === 2)
        return Promise.resolve(
          Response.json({ error: "A request conflicted" }, { status: 409 }),
        );
      if (bodies.length === 3) return originalReceipt.response;
    }
    throw new Error(
      "Unexpected request outside original metadata operation recovery",
    );
  });
  vi.stubGlobal("fetch", fetch);
  render(metadataPage());
  const metadata = within(
    screen.getByRole("region", { name: "Playlist metadata" }),
  );
  const table = screen.getByRole("table", { name: "Playlist entries" });
  const entries = within(table)
    .getAllByRole("row")
    .map((row) => row.textContent);
  const lookup = metadata.getByRole("button", { name: "Find missing albums" });
  fireEvent.click(lookup);
  await metadata.findByRole("alert");
  expect(bodies).toHaveLength(1);
  const body: RefreshImportedPlaylistMetadata = JSON.parse(bodies[0]);
  expect(body).toEqual({
    request_id: expect.any(String),
    installation_revision: metadataInstallation.revision,
  });
  await act(async () => {
    fireEvent.click(
      metadata.getByRole("button", { name: "Retry album lookup" }),
    );
  });
  expect(bodies).toEqual([bodies[0], bodies[0]]);
  const retry = metadata.getByRole("button", { name: "Retry album lookup" });
  expect(metadata.getByRole("alert").textContent).toMatch(
    /could not confirm.*retry the same request/i,
  );
  expect((lookup as HTMLButtonElement).disabled).toBe(true);
  expect((retry as HTMLButtonElement).disabled).toBe(false);
  expect(metadata.queryByRole("status")).toBeNull();
  expect(router.reload).not.toHaveBeenCalled();
  expect(settingsReads).toBe(1);
  await act(async () => {
    fireEvent.click(retry);
    fireEvent.click(retry);
  });
  expect(bodies).toEqual([bodies[0], bodies[0], bodies[0]]);
  expect((retry as HTMLButtonElement).disabled).toBe(true);
  expect(settingsReads).toBe(1);
  expect(router.reload).not.toHaveBeenCalled();
  await act(async () =>
    originalReceipt.resolve(
      Response.json({
        request_id: body.request_id,
        playlist_id: metadataProps.detail.playlist.id,
        updated_recordings: 1,
        unchanged_recordings: 1,
        unavailable_recordings: 0,
      } satisfies ImportedPlaylistMetadataRefresh),
    ),
  );
  expect(metadata.queryByRole("alert")).toBeNull();
  expect(
    metadata.queryByRole("button", { name: "Retry album lookup" }),
  ).toBeNull();
  expect(metadata.getByRole("status").textContent).toMatch(
    /1 recording updated.*1 unchanged.*0 unavailable/,
  );
  expect(router.reload).toHaveBeenCalledExactlyOnceWith(
    expect.objectContaining({ only: ["detail", "playlists"] }),
  );
  expect(settingsReads).toBe(1);
  expect(fetch.mock.calls.map(([path]) => path)).toEqual([
    metadataDownloadPath,
    "/api/v1/plugins",
    metadataPath,
    metadataPath,
    metadataPath,
  ]);
  expect(
    within(table)
      .getAllByRole("row")
      .map((row) => row.textContent),
  ).toEqual(entries);
  expect(
    screen.getByRole("heading", { name: metadataProps.detail.playlist.name }),
  ).toBeTruthy();
  expect(play).not.toHaveBeenCalled();
  expect(router.visit).not.toHaveBeenCalled();
});
