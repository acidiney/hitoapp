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
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import AudiobookPage from "../pages/AudiobookPage";
import {
  bookmarkPage,
  bookmarkResult,
  enterBookmark,
} from "../fixtures/audiobookBookmarks";
import { heldAudiobookBody } from "../fixtures/audiobooks";
import type { SyncBatch } from "../../../shared/types";

const boundaries = vi.hoisted(() => ({
  prepareSelection: vi.fn(() => {
    throw new Error("Bookmark revalidation must not open playback");
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
  localStorage.clear();
  vi.stubGlobal("fetch", request);
  vi.stubGlobal("Audio", audio);
});
afterEach(() => {
  cleanup();
  expect(boundaries.prepareSelection).not.toHaveBeenCalled();
  expect(audio).not.toHaveBeenCalled();
  expect(boundaries.visit).not.toHaveBeenCalled();
  expect(boundaries.reload).not.toHaveBeenCalled();
  expect(document.querySelector("audio")).toBeNull();
});

test("stale same-book props retain a checked bookmark until an authoritative empty list replaces it", async () => {
  const page = bookmarkPage();
  const label = "Confirmed private bookmark must survive old page props";
  const empty = heldAudiobookBody({ ...page.detail, bookmarks: [] });
  let batch: SyncBatch | undefined;
  let reads = 0;
  request.mockImplementation(async (input, options) => {
    expect(options?.credentials).toBe("same-origin");
    expect(new Headers(options?.headers).get("x-hito-account")).toBe(
      page.user.id,
    );
    expect(new Headers(options?.headers).get("x-hito-request")).toBe("1");
    expect(options?.signal).toBeInstanceOf(AbortSignal);
    if (input === "/api/v1/sync" && options?.method === "POST" && !batch) {
      batch = JSON.parse(String(options.body)) as SyncBatch;
      return Response.json({
        acknowledged: batch.operations.map((operation) => operation.id),
      });
    }
    if (
      input === `/api/v1/audiobooks/${page.detail.playlist.id}` &&
      options?.method === "GET" &&
      batch
    ) {
      reads += 1;
      if (reads === 1) return Response.json(bookmarkResult(page.detail, batch));
      if (reads === 2) return empty.response;
    }
    throw new Error("Unexpected bookmark freshness request");
  });
  let view: ReturnType<typeof render> | undefined;
  try {
    view = render(createElement(AudiobookPage, page));
    expect(request).not.toHaveBeenCalled();
    enterBookmark(page, label);
    fireEvent.click(screen.getByRole("button", { name: "Save bookmark" }));
    const markers = screen.getByRole("region", { name: "Bookmarks" });
    await within(markers).findByText(label, { exact: true });
    expect(request).toHaveBeenCalledTimes(2);

    view.rerender(
      createElement(AudiobookPage, {
        ...page,
        detail: {
          ...page.detail,
          playlist: { ...page.detail.playlist },
          chapters: page.detail.chapters.map((entry) => ({ ...entry })),
          bookmarks: page.detail.bookmarks.map((bookmark) => ({ ...bookmark })),
        },
      }),
    );
    expect(within(markers).getByText(label, { exact: true })).toBeTruthy();
    await waitFor(() => expect(empty.response.bodyUsed).toBe(true));
    expect(within(markers).getAllByRole("listitem")).toHaveLength(3);
    expect(within(markers).getByText(label, { exact: true })).toBeTruthy();
    await act(async () => empty.release());
    await waitFor(() =>
      expect(within(markers).queryByRole("listitem")).toBeNull(),
    );
    expect(within(markers).getByText("No bookmarks yet.")).toBeTruthy();
    expect(within(markers).queryByText(label, { exact: true })).toBeNull();
    for (const bookmark of page.detail.bookmarks)
      expect(
        within(markers).queryByText(bookmark.label, { exact: true }),
      ).toBeNull();
    expect(
      request.mock.calls.map(([path, options]) => [path, options?.method]),
    ).toEqual([
      ["/api/v1/sync", "POST"],
      [`/api/v1/audiobooks/${page.detail.playlist.id}`, "GET"],
      [`/api/v1/audiobooks/${page.detail.playlist.id}`, "GET"],
    ]);
  } finally {
    view?.unmount();
    await act(async () => empty.dispose());
  }
});
