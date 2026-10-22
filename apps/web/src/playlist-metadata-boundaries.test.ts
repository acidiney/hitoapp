import { createElement, type ReactNode } from "react";
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";
import { router } from "@inertiajs/react";
import type {
  ImportedPlaylistMetadataRefresh,
  PluginSettings,
  RefreshImportedPlaylistMetadata,
} from "./types";
import {
  metadataDownloadPath,
  metadataInstallation,
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

beforeEach(() => {
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
});

test("album lookup rejects a retargeted original connection instead of substituting another installation", async () => {
  const retargeted: PluginSettings = {
    ...metadataSettings,
    installations: [
      {
        ...metadataInstallation,
        manifest: {
          ...metadataInstallation.manifest,
          id: "org.hito.unrelated-provider",
        },
      },
      {
        ...metadataInstallation,
        id: "different-approved-installation",
        revision: 99,
      },
    ],
  };
  let settingsReads = 0;
  const fetch = vi.fn((path: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    if (path === metadataDownloadPath && method === "GET")
      return Promise.resolve(
        Response.json({ error: "Not found" }, { status: 404 }),
      );
    if (path === "/api/v1/plugins" && method === "GET") {
      settingsReads++;
      return Promise.resolve(
        Response.json(settingsReads === 1 ? retargeted : metadataSettings),
      );
    }
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
    throw new Error(
      "Unexpected request outside original connection validation",
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
  expect(metadata.getByRole("alert").textContent).toMatch(
    /original playlist connection.*plugin settings/i,
  );
  expect(
    metadata.queryByRole("button", { name: "Retry album lookup" }),
  ).toBeNull();
  expect(metadata.queryByRole("status")).toBeNull();
  expect((lookup as HTMLButtonElement).disabled).toBe(false);
  expect(fetch.mock.calls.map(([path]) => path)).toEqual([
    metadataDownloadPath,
    "/api/v1/plugins",
  ]);
  expect(router.reload).not.toHaveBeenCalled();
  await act(async () => {
    fireEvent.click(lookup);
  });
  expect(fetch.mock.calls.map(([path]) => path)).toEqual([
    metadataDownloadPath,
    "/api/v1/plugins",
    "/api/v1/plugins",
    metadataPath,
  ]);
  expect(JSON.parse(String(fetch.mock.calls[3][1]?.body))).toEqual({
    request_id: expect.any(String),
    installation_revision: metadataInstallation.revision,
  });
  expect(metadata.queryByRole("alert")).toBeNull();
  expect(metadata.getByRole("status").textContent).toMatch(
    /1 recording updated/,
  );
  expect(router.reload).toHaveBeenCalledTimes(1);
  expect(
    within(table)
      .getAllByRole("row")
      .map((row) => row.textContent),
  ).toEqual(originalEntries);
  expect(HTMLMediaElement.prototype.play).not.toHaveBeenCalled();
  expect(router.visit).not.toHaveBeenCalled();
});

test("mismatched album lookup receipts remain unconfirmed until the same request recovers its own result", async () => {
  const bodies: string[] = [];
  const fetch = vi.fn((path: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    if (path === metadataDownloadPath && method === "GET")
      return Promise.resolve(
        Response.json({ error: "Not found" }, { status: 404 }),
      );
    if (path === "/api/v1/plugins" && method === "GET")
      return Promise.resolve(Response.json(metadataSettings));
    if (path === metadataPath && method === "POST") {
      bodies.push(String(options?.body));
      const body: RefreshImportedPlaylistMetadata = JSON.parse(
        String(options?.body),
      );
      const wrongRequest =
        body.request_id === "b7a91d74-f835-4a08-9f6c-b345855ca8e6"
          ? "183539cc-fe04-46b7-aec2-35fabbd432ee"
          : "b7a91d74-f835-4a08-9f6c-b345855ca8e6";
      return Promise.resolve(
        Response.json({
          request_id: bodies.length === 1 ? wrongRequest : body.request_id,
          playlist_id:
            bodies.length === 2
              ? "b375e161-4dbb-48b3-a7dc-aec8a940117e"
              : metadataProps.detail.playlist.id,
          updated_recordings: bodies.length < 3 ? 999 : 1,
          unchanged_recordings: 1,
          unavailable_recordings: 0,
        } satisfies ImportedPlaylistMetadataRefresh),
      );
    }
    throw new Error("Unexpected request outside metadata receipt validation");
  });
  vi.stubGlobal("fetch", fetch);
  render(metadataPage());
  const metadata = within(
    screen.getByRole("region", { name: "Playlist metadata" }),
  );
  const table = screen.getByRole("table", { name: "Playlist entries" });
  const originalEntries = within(table)
    .getAllByRole("row")
    .map((row) => row.textContent);
  fireEvent.click(
    metadata.getByRole("button", { name: "Find missing albums" }),
  );
  await metadata.findByRole("alert");
  for (let rejected = 1; rejected <= 2; rejected++) {
    expect(bodies).toHaveLength(rejected);
    expect(metadata.getByRole("alert").textContent).toMatch(
      /could not confirm.*retry the same request/i,
    );
    expect(metadata.queryByRole("status")).toBeNull();
    expect(
      (
        metadata.getByRole("button", {
          name: "Find missing albums",
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(true);
    expect(router.reload).not.toHaveBeenCalled();
    await act(async () => {
      fireEvent.click(
        metadata.getByRole("button", { name: "Retry album lookup" }),
      );
    });
  }
  expect(bodies).toHaveLength(3);
  expect(bodies).toEqual([bodies[0], bodies[0], bodies[0]]);
  expect(metadata.queryByRole("alert")).toBeNull();
  expect(
    metadata.queryByRole("button", { name: "Retry album lookup" }),
  ).toBeNull();
  expect(metadata.getByRole("status").textContent).toMatch(
    /1 recording updated.*1 unchanged/,
  );
  expect(router.reload).toHaveBeenCalledExactlyOnceWith(
    expect.objectContaining({ only: ["detail", "playlists"] }),
  );
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
  ).toEqual(originalEntries);
  expect(HTMLMediaElement.prototype.play).not.toHaveBeenCalled();
  expect(router.visit).not.toHaveBeenCalled();
});
