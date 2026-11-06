import { createElement, type ReactNode } from "react";
import { render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import PlaylistPage from "../pages/PlaylistPage";
import { NoticeProvider } from "../../../shared/ui/Notices";
import { PlaybackProvider } from "../../playback/components/PlaybackProvider";
import { copiedPlaylistPage as props } from "../fixtures/playlistPages";

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

test("playlist entries show an album column whose names link to the artist and album pages", () => {
  vi.stubGlobal(
    "fetch",
    vi.fn(async () => Response.json({}, { status: 404 })),
  );
  render(
    createElement(
      NoticeProvider,
      null,
      createElement(PlaybackProvider, {
        account: props.user.id,
        children: createElement(PlaylistPage, props),
      }),
    ),
  );
  const table = screen.getByRole("table");
  const headers = within(table)
    .getAllByRole("columnheader")
    .map((header) => header.textContent);
  expect(headers).toContain("Album");
  const row = within(within(table).getAllByRole("row")[1]);
  expect(
    row
      .getAllByRole("link", { name: "Fixture album" })
      .map((link) => link.getAttribute("href")),
  ).toContain("/albums/release-fixture");
  expect(
    row
      .getAllByRole("link", { name: "Fixture artist" })
      .map((link) => link.getAttribute("href")),
  ).toContain("/artists/artist-fixture");
});
