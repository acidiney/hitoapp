// @vitest-environment jsdom
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
import PlaylistIndexPage from "./pages/PlaylistIndexPage";
import { NoticeProvider } from "./components/Notices";
import type { PlaylistIndexPageProps, PluginInstallation } from "./types";
import type { components } from "./generated";

vi.mock("@inertiajs/react", () => ({
  router: { visit: vi.fn(), reload: vi.fn() },
  Link: ({ children, href }: { children: ReactNode; href: string }) =>
    createElement("a", { href }, children),
}));

const connection: PluginInstallation = {
  id: "playlist-installation",
  service_id: "playlist-service",
  revision: 3,
  checked_at: 1,
  credential_set: false,
  manifest: {
    id: "org.hito.playlist-fixture",
    name: "Playlist fixture",
    protocol_version: 1,
    capabilities: ["playlists"],
  },
};
const props: PlaylistIndexPageProps = {
  user: { id: "alice", username: "Alice", admin: false },
  errors: {},
  native_playback: false,
  libraries: [],
  playlists: [],
  connections: [
    {
      ...connection,
      id: "search-installation",
      service_id: "search-service",
      manifest: { ...connection.manifest, capabilities: ["search"] },
    },
    connection,
  ],
  service_names: {
    "search-service": "Search collection",
    "playlist-service": "Playlist collection",
  },
};

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  vi.clearAllMocks();
});

test("Playlists imports an editable provider copy and navigates only after its receipt", async () => {
  let resolve!: (response: Response) => void;
  const response = new Promise<Response>((done) => {
    resolve = done;
  });
  const fetch = vi.fn().mockReturnValue(response);
  vi.stubGlobal("fetch", fetch);
  render(createElement(PlaylistIndexPage, props), {
    wrapper: ({ children }) => createElement(NoticeProvider, null, children),
  });
  const form = within(
    screen.getByRole("region", { name: "Import a playlist" }),
  );
  const services = form.getByRole("combobox", { name: "Playlist service" });
  expect(form.queryByRole("option", { name: /Search collection/ })).toBeNull();
  fireEvent.change(services, { target: { value: connection.id } });
  const reference =
    "https://provider.invalid/playlists/private-pasted-reference";
  fireEvent.change(
    form.getByRole("textbox", { name: "Playlist link or identifier" }),
    { target: { value: `  ${reference}  ` } },
  );
  expect(fetch).not.toHaveBeenCalled();
  fireEvent.click(form.getByRole("button", { name: "Import a copy" }));
  await waitFor(() => expect(fetch).toHaveBeenCalledTimes(1));
  expect(fetch.mock.calls[0][0]).toBe(
    "/api/v1/plugins/playlist-installation/playlists/import",
  );
  expect(fetch.mock.calls[0][1]).toMatchObject({
    method: "POST",
    credentials: "same-origin",
    headers: { "x-hito-account": "alice" },
    signal: expect.any(AbortSignal),
  });
  expect(JSON.parse(fetch.mock.calls[0][1].body)).toEqual({
    request_id: expect.stringMatching(
      /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
    ),
    installation_revision: 3,
    playlist_reference: reference,
  });
  expect(
    (form.getByRole("button", { name: "Importing…" }) as HTMLButtonElement)
      .disabled,
  ).toBe(true);
  expect(router.visit).not.toHaveBeenCalled();
  const request = JSON.parse(fetch.mock.calls[0][1].body);
  const receipt = {
    request_id: request.request_id,
    playlist_id: "13d9f3b8-13ac-4413-acb1-316b9aba81be",
  } satisfies components["schemas"]["PlaylistImportReceipt"];
  await act(async () => resolve(new Response(JSON.stringify(receipt))));
  await waitFor(() =>
    expect(router.visit).toHaveBeenCalledExactlyOnceWith(
      `/playlists/${receipt.playlist_id}`,
    ),
  );
  expect(fetch).toHaveBeenCalledTimes(1);
  expect(screen.getByRole("button", { name: "Create playlist" })).toBeTruthy();
});
