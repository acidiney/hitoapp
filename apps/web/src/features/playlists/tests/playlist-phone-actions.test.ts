import { createElement, type ReactNode } from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import PlaylistPage from "../pages/PlaylistPage";
import { NoticeProvider } from "../../../shared/ui/Notices";
import { PlaybackProvider } from "../../playback/components/PlaybackProvider";
import { copiedPlaylistPage } from "../fixtures/playlistPages";

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({
    children,
    href,
    ...attributes
  }: {
    children: ReactNode;
    href: string;
  }) => createElement("a", { href, ...attributes }, children),
}));

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

function open(phone: boolean) {
  vi.stubGlobal("matchMedia", (query: string) => ({
    matches: phone && query.includes("767px"),
    addEventListener: () => {},
    removeEventListener: () => {},
  }));
  vi.stubGlobal(
    "fetch",
    vi.fn(async () => Response.json({ error: "Not found" }, { status: 404 })),
  );
  render(
    createElement(
      NoticeProvider,
      null,
      createElement(PlaybackProvider, {
        account: copiedPlaylistPage.user.id,
        children: createElement(PlaylistPage, copiedPlaylistPage),
      }),
    ),
  );
}

test("on a phone, Edit and Download move into the More menu so the header keeps one row", () => {
  open(true);
  expect(screen.queryByRole("button", { name: "Edit" })).toBeNull();
  expect(screen.queryByRole("button", { name: "Download" })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "More options" }));
  expect(screen.getByRole("menuitem", { name: "Edit" })).toBeTruthy();
  expect(screen.getByRole("menuitem", { name: "Download" })).toBeTruthy();
});

test("on a wider screen, Edit and Download stay as header buttons", () => {
  open(false);
  expect(screen.getByRole("button", { name: "Edit" })).toBeTruthy();
  expect(screen.getByRole("button", { name: "Download" })).toBeTruthy();
});
