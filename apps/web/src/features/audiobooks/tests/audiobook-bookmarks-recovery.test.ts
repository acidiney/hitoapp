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
    throw new Error("Bookmark recovery must not open playback");
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
  vi.spyOn(console, "info").mockImplementation(() => {});
  vi.spyOn(console, "warn").mockImplementation(() => {});
  vi.spyOn(console, "debug").mockImplementation(() => {});
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

test.each(["acknowledgement", "read"] as const)(
  "a failed bookmark %s recovers the original operation without an early saved claim",
  async (failure) => {
    const page = bookmarkPage();
    const label = `Private bookmark ${failure} recovery`;
    const diagnostics = "Private interrupted bookmark connection diagnostics";
    const writes: string[] = [];
    let reads = 0;
    let held: ReturnType<typeof heldAudiobookBody> | undefined;
    request.mockImplementation(async (input, options) => {
      expect(options?.credentials).toBe("same-origin");
      expect(new Headers(options?.headers).get("x-hito-account")).toBe(
        page.user.id,
      );
      expect(new Headers(options?.headers).get("x-hito-request")).toBe("1");
      expect(options?.signal).toBeInstanceOf(AbortSignal);
      if (input === "/api/v1/sync" && options?.method === "POST") {
        writes.push(String(options.body));
        const batch = JSON.parse(writes[0]) as SyncBatch;
        if (failure === "acknowledgement" && writes.length === 1)
          throw new TypeError(diagnostics);
        return Response.json({
          acknowledged: batch.operations.map((operation) => operation.id),
        });
      }
      if (
        input === `/api/v1/audiobooks/${page.detail.playlist.id}` &&
        options?.method === "GET"
      ) {
        reads += 1;
        if (failure === "read" && reads === 1) throw new TypeError(diagnostics);
        held = heldAudiobookBody(
          bookmarkResult(page.detail, JSON.parse(writes[0]) as SyncBatch),
        );
        return held.response;
      }
      throw new Error("Unexpected bookmark recovery request");
    });
    let view: ReturnType<typeof render> | undefined;
    try {
      view = render(createElement(AudiobookPage, page));
      expect(request).not.toHaveBeenCalled();
      enterBookmark(page, label);
      fireEvent.click(screen.getByRole("button", { name: "Save bookmark" }));
      const alert = await screen.findByRole("alert");
      expect(alert.textContent?.includes(diagnostics)).toBe(false);
      expect(writes).toHaveLength(1);
      const interruptedRequests = request.mock.calls.length;
      view.rerender(
        createElement(AudiobookPage, {
          ...page,
          detail: {
            ...page.detail,
            bookmarks: page.detail.bookmarks.map((bookmark) => ({
              ...bookmark,
            })),
          },
        }),
      );
      expect(request).toHaveBeenCalledTimes(interruptedRequests);
      const retry = screen.getByRole("button", {
        name:
          failure === "acknowledgement"
            ? "Retry bookmark change"
            : "Check bookmarks",
      });
      expect(
        screen.queryByRole("button", {
          name:
            failure === "acknowledgement"
              ? "Check bookmarks"
              : "Retry bookmark change",
        }),
      ).toBeNull();
      const markers = screen.getByRole("region", { name: "Bookmarks" });
      expect(within(markers).queryByText(label, { exact: true })).toBeNull();
      expect(screen.queryByText("Bookmark saved.", { exact: true })).toBeNull();
      fireEvent.click(retry);
      fireEvent.click(retry);
      await waitFor(() => expect(held?.response.bodyUsed).toBe(true));
      if (failure === "acknowledgement") {
        expect(writes).toHaveLength(2);
        expect(writes[1]).toBe(writes[0]);
      } else expect(writes).toHaveLength(1);
      expect(within(markers).queryByText(label, { exact: true })).toBeNull();
      expect(screen.queryByText("Bookmark saved.", { exact: true })).toBeNull();
      await act(async () => held?.release());
      await within(markers).findByText(label, { exact: true });
      expect(within(markers).getAllByRole("listitem")).toHaveLength(3);
      expect(screen.getByText("Bookmark saved.", { exact: true })).toBeTruthy();
      const original = JSON.parse(writes[0]) as SyncBatch;
      expect(original.operations).toHaveLength(1);
      expect(original.operations[0].action).toEqual({
        kind: "audiobook_bookmark_create",
        audiobook_id: page.detail.playlist.id,
        bookmark_id: expect.any(String),
        entry_id: page.detail.chapters[2].entry_id,
        recording_id: page.detail.chapters[2].recording_id,
        position_ms: 35_500,
        label,
      });
      const read = [`/api/v1/audiobooks/${page.detail.playlist.id}`, "GET"];
      const write = ["/api/v1/sync", "POST"];
      expect(
        request.mock.calls.map(([path, options]) => [path, options?.method]),
      ).toEqual(
        failure === "acknowledgement"
          ? [write, write, read]
          : [write, read, read],
      );
      const logs = JSON.stringify(
        [console.info, console.warn, console.debug].flatMap(
          (method) => vi.mocked(method).mock.calls,
        ),
      );
      expect(logs.includes(label)).toBe(false);
      expect(logs.includes(diagnostics)).toBe(false);
    } finally {
      view?.unmount();
      await act(async () => held?.dispose());
    }
  },
);
