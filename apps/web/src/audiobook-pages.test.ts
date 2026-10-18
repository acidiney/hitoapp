// @vitest-environment jsdom
import { createElement, type ReactNode } from "react";
import { cleanup, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import AudiobookIndexPage from "./pages/AudiobookIndexPage";
import AudiobookPage from "./pages/AudiobookPage";
import { audiobookPages } from "./fixtures/audiobooks";

const boundaries = vi.hoisted(() => ({
  player: vi.fn(() => {
    throw new Error("Read-only audiobook pages must not open playback");
  }),
  visit: vi.fn(),
  reload: vi.fn(),
}));
vi.mock("./player/PlaybackProvider", () => ({ usePlayer: boundaries.player }));
vi.mock("@inertiajs/react", () => ({
  router: { visit: boundaries.visit, reload: boundaries.reload },
  Link: ({
    children,
    href,
    ...attributes
  }: {
    children: ReactNode;
    href: string;
  }) => createElement("a", { href, ...attributes }, children),
}));

const request = vi.fn<typeof globalThis.fetch>();
const audio = vi.fn();

beforeEach(() => {
  vi.clearAllMocks();
  request.mockRejectedValue(
    new Error("Unexpected audiobook rendering request"),
  );
  audio.mockImplementation(() => {
    throw new Error("Read-only audiobook pages must not create audio");
  });
  vi.stubGlobal("fetch", request);
  vi.stubGlobal("Audio", audio);
});
afterEach(() => {
  cleanup();
  try {
    expect(request).not.toHaveBeenCalled();
    expect(audio).not.toHaveBeenCalled();
    expect(boundaries.player).not.toHaveBeenCalled();
    expect(boundaries.visit).not.toHaveBeenCalled();
    expect(boundaries.reload).not.toHaveBeenCalled();
    expect(document.querySelector("audio")).toBeNull();
  } finally {
    vi.unstubAllGlobals();
  }
});

test("the audiobook index links only grouped books and explains an empty collection", () => {
  const { index } = audiobookPages();
  const view = render(createElement(AudiobookIndexPage, index));
  expect(
    screen.getByRole("heading", { level: 1, name: "Audiobooks" }),
  ).toBeTruthy();
  expect(
    screen
      .getByRole("link", { name: index.books[0].name })
      .getAttribute("href"),
  ).toBe(`/audiobooks/${index.books[0].id}`);
  expect(
    screen.queryByRole("link", { name: "Ungrouped selection" }),
  ).toBeNull();
  expect(
    screen.queryByRole("heading", { name: "No audiobooks yet" }),
  ).toBeNull();

  view.rerender(createElement(AudiobookIndexPage, { ...index, books: [] }));
  expect(
    screen.getByRole("heading", { name: "No audiobooks yet" }),
  ).toBeTruthy();
  expect(
    screen.getByRole("link", { name: "Browse playlists" }).getAttribute("href"),
  ).toBe("/playlists");
  expect(screen.queryByRole("link", { name: index.books[0].name })).toBeNull();
});

test("chapter order and bookmarks retain the exact repeated recording occurrence", () => {
  const { detail, bookmark } = audiobookPages();
  render(createElement(AudiobookPage, detail));
  expect(
    screen.getByRole("heading", {
      level: 1,
      name: detail.detail.playlist.name,
    }),
  ).toBeTruthy();
  expect(
    screen.getByRole("link", { name: "All audiobooks" }).getAttribute("href"),
  ).toBe("/audiobooks");
  expect(
    screen.getByRole("link", { name: "Open playlist" }).getAttribute("href"),
  ).toBe(`/playlists/${detail.detail.playlist.id}`);
  const rows = within(screen.getByRole("table", { name: "Chapters" }))
    .getAllByRole("row")
    .slice(1);
  expect(rows).toHaveLength(4);
  expect(rows.map((row) => row.id)).toEqual(
    detail.detail.chapters.map((chapter) => `chapter-${chapter.entry_id}`),
  );
  for (const [index, chapter] of detail.detail.chapters.entries()) {
    const row = within(rows[index]);
    expect(
      row.getByRole("rowheader", { name: `Chapter ${index + 1}` }),
    ).toBeTruthy();
    expect(row.getByText(chapter.title, { exact: true })).toBeTruthy();
  }
  expect(
    rows
      .slice(0, 3)
      .map((row) => within(row).getByText(/Owned chapter [AB]/).textContent),
  ).toEqual(["Owned chapter A", "Owned chapter B", "Owned chapter A"]);

  const markers = within(screen.getByRole("region", { name: "Bookmarks" }));
  const saved = markers.getAllByRole("listitem");
  expect(saved).toHaveLength(1);
  expect(
    within(saved[0]).getByText(bookmark.label, { exact: true }),
  ).toBeTruthy();
  expect(
    within(saved[0]).getByText("Chapter 3: Owned chapter A", { exact: true }),
  ).toBeTruthy();
  expect(within(saved[0]).getByText("0:35", { exact: true })).toBeTruthy();
  expect(
    within(saved[0]).queryByText("Chapter 1: Owned chapter A", { exact: true }),
  ).toBeNull();
});

test("retained unavailable chapters and removed chapter bookmarks never retarget a namesake recording", () => {
  const { detail, bookmark, removedChapterBookmark } = audiobookPages();
  render(
    createElement(AudiobookPage, {
      ...detail,
      detail: {
        ...detail.detail,
        bookmarks: [bookmark, removedChapterBookmark],
      },
    }),
  );
  const rows = within(screen.getByRole("table", { name: "Chapters" }))
    .getAllByRole("row")
    .slice(1);
  const retained = within(rows[3]);
  expect(retained.getByText("Retained chapter", { exact: true })).toBeTruthy();
  expect(
    retained.getByText("Chapter unavailable", { exact: true }),
  ).toBeTruthy();
  expect(
    retained.queryByRole("button", { name: /Play|Resume|Start over/ }),
  ).toBeNull();
  expect(retained.queryByRole("link", { name: /Details/ })).toBeNull();

  const markers = within(
    screen.getByRole("region", { name: "Bookmarks" }),
  ).getAllByRole("listitem");
  expect(markers).toHaveLength(2);
  const orphan = within(markers[1]);
  expect(
    orphan.getByText(removedChapterBookmark.label, { exact: true }),
  ).toBeTruthy();
  expect(orphan.getByText("0:15", { exact: true })).toBeTruthy();
  expect(orphan.getByText("Chapter unavailable", { exact: true })).toBeTruthy();
  expect(orphan.queryByText(/Chapter [13]: Owned chapter A/)).toBeNull();
  expect(orphan.queryByRole("link")).toBeNull();
  expect(orphan.queryByRole("button", { name: /Play|Resume|Jump/ })).toBeNull();
  expect(
    screen.queryByRole("button", { name: /Download|Continue/ }),
  ).toBeNull();
});
