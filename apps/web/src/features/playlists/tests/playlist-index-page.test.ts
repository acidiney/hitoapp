import type { Schema } from "../../../shared/schema";
import { createElement, type ReactNode } from "react";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { router } from "@inertiajs/react";
import PlaylistIndexPage from "../pages/PlaylistIndexPage";
import { NoticeBanner, NoticeProvider } from "../../../shared/ui/Notices";
import { change } from "../../../shared/sync";
import { playlistImportPage } from "../fixtures/playlistImports";

vi.mock("../../../shared/sync", () => ({ change: vi.fn() }));
vi.mock("@inertiajs/react", () => ({
  router: { visit: vi.fn(), reload: vi.fn() },
  Link: ({ children, href }: { children: ReactNode; href: string }) =>
    createElement("a", { href }, children),
}));

const wrapper = ({ children }: { children: ReactNode }) =>
  createElement(NoticeProvider, null, children, createElement(NoticeBanner));
const empty = playlistImportPage;
const populated: Schema<"PlaylistIndexPageProps"> = {
  ...empty,
  playlists: [
    { id: "road", name: "Road trip", kind: "local" },
    { id: "late", name: "Late night", kind: "local" },
  ],
};

afterEach(() => {
  cleanup();
  window.location.hash = "";
  vi.clearAllMocks();
});

test("an empty Playlists page is one empty state with a primary and a secondary action", () => {
  render(createElement(PlaylistIndexPage, empty), { wrapper });
  expect(screen.getAllByRole("heading", { level: 2 })).toHaveLength(1);
  expect(screen.queryByText("A home for your favorites")).toBeNull();
  expect(screen.queryByRole("textbox")).toBeNull();
  expect(
    screen.queryByRole("region", { name: "Import a playlist" }),
  ).toBeNull();
  const create = screen.getByRole("button", { name: "New playlist" });
  const link = screen.getByRole("button", { name: "Import from a link" });
  expect(create.className).toContain("primary");
  expect(link.className).not.toContain("primary");
});

test("a populated page offers the same two actions in its header instead of two stacked forms", () => {
  render(createElement(PlaylistIndexPage, populated), { wrapper });
  expect(screen.getByRole("button", { name: "New playlist" })).toBeTruthy();
  expect(
    screen.getByRole("button", { name: "Import from a link" }),
  ).toBeTruthy();
  expect(screen.queryByRole("textbox")).toBeNull();
  expect(
    screen.getAllByRole("link", { name: /Road trip|Late night/ }),
  ).toHaveLength(2);
});

test("New playlist opens a dialog that creates the playlist and reloads", async () => {
  vi.mocked(change).mockResolvedValueOnce({ acknowledged: [] });
  render(createElement(PlaylistIndexPage, empty), { wrapper });
  fireEvent.click(screen.getByRole("button", { name: "New playlist" }));
  const dialog = within(screen.getByRole("dialog", { name: "New playlist" }));
  fireEvent.change(dialog.getByRole("textbox", { name: "Name" }), {
    target: { value: "Evening" },
  });
  fireEvent.click(dialog.getByRole("button", { name: "Create playlist" }));
  await waitFor(() => expect(router.reload).toHaveBeenCalledTimes(1));
  expect(vi.mocked(change).mock.calls[0][1]).toMatchObject({
    kind: "playlist_create",
    name: "Evening",
  });
  expect(screen.queryByRole("dialog")).toBeNull();
});

test("a failed creation keeps the dialog and the typed name", async () => {
  vi.mocked(change).mockRejectedValueOnce(new Error("Server unavailable"));
  render(createElement(PlaylistIndexPage, empty), { wrapper });
  fireEvent.click(screen.getByRole("button", { name: "New playlist" }));
  const input = screen.getByRole("textbox", {
    name: "Name",
  }) as HTMLInputElement;
  fireEvent.change(input, { target: { value: "Keep this" } });
  fireEvent.click(screen.getByRole("button", { name: "Create playlist" }));
  await screen.findByText("Server unavailable");
  expect(input.value).toBe("Keep this");
  expect(router.reload).not.toHaveBeenCalled();
});

test("a name you already use gets a gentle warning but can still be created", async () => {
  vi.mocked(change).mockResolvedValueOnce({ acknowledged: [] });
  render(createElement(PlaylistIndexPage, populated), { wrapper });
  fireEvent.click(screen.getByRole("button", { name: "New playlist" }));
  const input = screen.getByRole("textbox", { name: "Name" });
  expect(screen.queryByRole("status")).toBeNull();
  fireEvent.change(input, { target: { value: "  road TRIP " } });
  expect(screen.getByRole("status").textContent).toBe(
    "You already have a playlist called “road TRIP”. You can still create another.",
  );
  const create = screen.getByRole("button", {
    name: "Create playlist",
  }) as HTMLButtonElement;
  expect(create.disabled).toBe(false);
  fireEvent.click(create);
  await waitFor(() => expect(router.reload).toHaveBeenCalledTimes(1));
});

test("Cancel closes the dialog without creating anything", () => {
  render(createElement(PlaylistIndexPage, empty), { wrapper });
  fireEvent.click(screen.getByRole("button", { name: "New playlist" }));
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(change).not.toHaveBeenCalled();
});

test("the sidebar's create link opens the dialog", () => {
  window.location.hash = "#new-playlist";
  render(createElement(PlaylistIndexPage, populated), { wrapper });
  expect(screen.getByRole("dialog", { name: "New playlist" })).toBeTruthy();
});

test("Import from a link opens the import dialog with a link field and a hint about the link", () => {
  render(createElement(PlaylistIndexPage, empty), { wrapper });
  fireEvent.click(screen.getByRole("button", { name: "Import from a link" }));
  const dialog = screen.getByRole("dialog", { name: "Import a playlist" });
  const region = within(dialog).getByRole("region", {
    name: "Import a playlist",
  });
  expect(
    within(region).getByRole("textbox", {
      name: "Playlist link or identifier",
    }),
  ).toBeTruthy();
  expect(
    within(region).getByText(/A link looks like open\.spotify\.com\/playlist/),
  ).toBeTruthy();
  expect(within(dialog).getByRole("button", { name: "Cancel" })).toBeTruthy();
});

test("without a playlist service the import dialog only offers to connect one", () => {
  render(createElement(PlaylistIndexPage, { ...empty, connections: [] }), {
    wrapper,
  });
  fireEvent.click(screen.getByRole("button", { name: "Import from a link" }));
  const region = within(
    screen.getByRole("dialog", { name: "Import a playlist" }),
  ).getByRole("region", { name: "Import a playlist" });
  expect([...region.querySelectorAll("p")].map((p) => p.textContent)).toEqual([
    "Connect a playlist service to copy a playlist into Hito.",
  ]);
  expect(
    within(region).getByRole("link", { name: "Connect a playlist service" }),
  ).toBeTruthy();
});
