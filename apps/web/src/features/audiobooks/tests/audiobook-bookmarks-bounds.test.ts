import { createElement, type ReactNode } from "react";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import AudiobookPage from "../pages/AudiobookPage";
import { bookmarkPage, enterBookmark } from "../fixtures/audiobookBookmarks";

const boundaries = vi.hoisted(() => ({
  prepareSelection: vi.fn(() => {
    throw new Error("Rejected bookmarks must not open playback");
  }),
  visit: vi.fn(),
  reload: vi.fn(),
}));
vi.mock("../../playback/components/PlaybackProvider", () => ({
  usePlayer: () => ({
    mode: "browser",
    prepareSelection: boundaries.prepareSelection,
  }),
}));
vi.mock("@inertiajs/react", () => ({
  router: { visit: boundaries.visit, reload: boundaries.reload },
  Link: ({ children, href }: { children: ReactNode; href: string }) =>
    createElement("a", { href }, children),
}));

const request = vi.fn<typeof globalThis.fetch>();
const audio = vi.fn();
beforeEach(() => {
  vi.clearAllMocks();
  vi.spyOn(console, "warn").mockImplementation(() => {});
  localStorage.clear();
  request.mockRejectedValue(
    new Error("Invalid bookmarks must not request data"),
  );
  vi.stubGlobal("fetch", request);
  vi.stubGlobal("Audio", audio);
});
afterEach(() => {
  cleanup();
  expect(request).not.toHaveBeenCalled();
  expect(boundaries.prepareSelection).not.toHaveBeenCalled();
  expect(audio).not.toHaveBeenCalled();
  expect(boundaries.visit).not.toHaveBeenCalled();
  expect(boundaries.reload).not.toHaveBeenCalled();
  expect(document.querySelector("audio")).toBeNull();
});

test.each([
  {
    name: "a label over 256 UTF-8 bytes",
    label: "é".repeat(129),
    position: "35.5",
  },
  {
    name: "raw label bytes before trimming",
    label: `${" ".repeat(256)}x`,
    position: "35.5",
  },
  { name: "an empty trimmed label", label: "   ", position: "35.5" },
  {
    name: "four fractional position digits",
    label: "Private precision note",
    position: "35.5001",
  },
  {
    name: "a negative position",
    label: "Private negative position note",
    position: "-1",
  },
  {
    name: "an unsafe integer millisecond position",
    label: "Private oversized position note",
    position: "9007199254740.992",
  },
])(
  "$name is rejected without changing private bookmarks or performing I/O",
  ({ label, position }) => {
    const page = bookmarkPage();
    render(createElement(AudiobookPage, page));
    enterBookmark(page, label, position);
    const form = screen
      .getByRole("button", { name: "Save bookmark" })
      .closest("form");
    if (!form) throw new Error("Bookmark creation requires a form");
    fireEvent.submit(form);
    expect(
      screen.getByRole("alert").textContent?.trim().length,
    ).toBeGreaterThan(0);
    const markers = screen.getByRole("region", { name: "Bookmarks" });
    expect(within(markers).getAllByRole("listitem")).toHaveLength(
      page.detail.bookmarks.length,
    );
    for (const bookmark of page.detail.bookmarks)
      expect(
        within(markers).getByText(bookmark.label, { exact: true }),
      ).toBeTruthy();
    expect(screen.queryByText("Bookmark saved.", { exact: true })).toBeNull();
    const logs = JSON.stringify(vi.mocked(console.warn).mock.calls);
    expect(logs.includes(label)).toBe(false);
  },
);

test("an unavailable chapter cannot receive a new bookmark while its retained markers remain readable", () => {
  const page = bookmarkPage();
  const unavailable = page.detail.chapters[3];
  render(createElement(AudiobookPage, page));
  const select = screen.getByRole("combobox", { name: "Chapter" });
  const option = within(select).getByRole("option", {
    name: `Chapter 4: ${unavailable.title}`,
  }) as HTMLOptionElement;
  expect(option.disabled).toBe(true);
  enterBookmark(
    page,
    "Private unavailable chapter note",
    "35.5",
    unavailable.entry_id,
  );
  const form = screen
    .getByRole("button", { name: "Save bookmark" })
    .closest("form");
  if (!form) throw new Error("Bookmark creation requires a form");
  fireEvent.submit(form);
  expect(screen.getByRole("alert")).toBeTruthy();
  const markers = screen.getByRole("region", { name: "Bookmarks" });
  expect(within(markers).getAllByRole("listitem")).toHaveLength(
    page.detail.bookmarks.length,
  );
  expect(
    within(markers).getByText("Chapter unavailable", { exact: true }),
  ).toBeTruthy();
});
