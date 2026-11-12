import { createElement, type ReactNode } from "react";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";
import SearchBar from "../components/SearchBar";
import { pluginsApi } from "../../plugins/api/pluginsApi";

vi.mock("@inertiajs/react", () => ({
  usePage: () => ({ props: { user: { id: "alice" } } }),
  router: { visit: vi.fn() },
  Link: ({ href, children }: { href: string; children: ReactNode }) =>
    createElement("a", { href }, children),
}));
vi.mock("../../plugins/api/pluginsApi", () => ({
  pluginsApi: { settings: vi.fn() },
}));

beforeEach(() => {
  vi.mocked(pluginsApi.settings).mockResolvedValue({
    credential_storage_ready: true,
    services: [
      {
        id: "svc",
        name: "Spotify public playlists",
        base_url: "x",
        revision: 1,
      },
    ],
    installations: [
      {
        id: "installation",
        service_id: "svc",
        revision: 1,
        checked_at: 1,
        credential_set: true,
        manifest: {
          id: "org.hito.spotify",
          name: "Spotify public playlists",
          protocol_version: 1,
          capabilities: ["playlists"],
        },
      },
    ],
  });
});

function field() {
  return screen.getByPlaceholderText("Search, or paste a playlist link");
}

test("a pasted playlist link is recognised before Enter and offers to open the import dialog", () => {
  const onSubmit = vi.fn();
  render(createElement(SearchBar, { value: "", onChange: vi.fn(), onSubmit }));
  expect(screen.queryByRole("status")).toBeNull();
  render(
    createElement(SearchBar, {
      value: "https://open.spotify.com/playlist/37i9dQ",
      onChange: vi.fn(),
      onSubmit,
    }),
  );
  const hint = screen.getByRole("status");
  expect(within(hint).getByText("Spotify playlist link")).toBeTruthy();
  expect(within(hint).getByText("Follow it or copy it")).toBeTruthy();
});

test("Enter on a playlist link opens the import dialog with the link filled in instead of searching", async () => {
  const onSubmit = vi.fn();
  const link = "https://open.spotify.com/playlist/37i9dQ";
  render(
    createElement(SearchBar, { value: link, onChange: vi.fn(), onSubmit }),
  );
  fireEvent.submit(field().closest("form") as HTMLFormElement);
  const dialog = await screen.findByRole("dialog", {
    name: "Import a playlist",
  });
  const input = await within(dialog).findByRole("textbox", {
    name: "Playlist link or identifier",
  });
  expect((input as HTMLInputElement).value).toBe(link);
  expect(onSubmit).not.toHaveBeenCalled();
});

test("Enter on ordinary words still searches", () => {
  const onSubmit = vi.fn();
  render(
    createElement(SearchBar, { value: "stromae", onChange: vi.fn(), onSubmit }),
  );
  fireEvent.submit(field().closest("form") as HTMLFormElement);
  expect(onSubmit).toHaveBeenCalledWith("stromae");
  expect(screen.queryByRole("dialog")).toBeNull();
});
