import { createElement, type ReactNode } from "react";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import PlaylistImport from "../components/PlaylistImport";
import { NoticeProvider } from "../../../shared/ui/Notices";
import { playlistConnection } from "../fixtures/playlistImports";

vi.mock("@inertiajs/react", () => ({
  router: { visit: vi.fn(), reload: vi.fn() },
  Link: ({ children, href }: { children: ReactNode; href: string }) =>
    createElement("a", { href }, children),
}));

const named = (id: string, name: string) => ({
  ...playlistConnection,
  id: `${id}-installation`,
  service_id: `${id}-service`,
  manifest: { ...playlistConnection.manifest, id: `org.example.${id}`, name },
});
const spotify = named("spotify", "Spotify");
const youtube = named("youtube", "YouTube");

function view(connections = [spotify, youtube]) {
  return render(
    createElement(
      NoticeProvider,
      null,
      createElement(PlaylistImport, {
        account: "alice",
        connections,
        names: {},
      }),
    ),
  );
}
const field = () =>
  screen.getByRole("textbox", { name: "Playlist link or identifier" });

test("the service select is shown only while the pasted text does not choose a service", () => {
  view();
  expect(
    screen.getByRole("combobox", { name: "Playlist service" }),
  ).toBeTruthy();
  fireEvent.change(field(), {
    target: { value: "https://open.spotify.com/playlist/37i9dQ" },
  });
  expect(
    screen.queryByRole("combobox", { name: "Playlist service" }),
  ).toBeNull();
});

test("a single connected service never asks for a choice", () => {
  view([spotify]);
  expect(
    screen.queryByRole("combobox", { name: "Playlist service" }),
  ).toBeNull();
});

test("a recognised provider link follows by default and goes to that service", async () => {
  const fetch = vi.fn().mockReturnValue(new Promise(() => {}));
  vi.stubGlobal("fetch", fetch);
  view();
  fireEvent.change(field(), {
    target: { value: "https://music.youtube.com/playlist?list=PL1" },
  });
  expect(
    screen.getByRole("radio", { name: "Follow playlist", checked: true }),
  ).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Follow playlist" }));
  await waitFor(() => expect(fetch).toHaveBeenCalled());
  expect(fetch.mock.calls[0][0]).toBe(
    "/api/v1/plugins/youtube-installation/playlists/follow",
  );
});

test("choosing Import a copy sticks even for a provider link", () => {
  view();
  fireEvent.click(screen.getByRole("radio", { name: "Import a copy" }));
  fireEvent.change(field(), {
    target: { value: "https://open.spotify.com/playlist/37i9dQ" },
  });
  expect(
    screen.getByRole("radio", { name: "Import a copy", checked: true }),
  ).toBeTruthy();
  expect(screen.getByRole("button", { name: "Import a copy" })).toBeTruthy();
});
