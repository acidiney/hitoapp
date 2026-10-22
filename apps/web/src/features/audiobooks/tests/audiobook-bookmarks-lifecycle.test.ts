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
import type { AudiobookPageProps, SyncBatch } from "../../../shared/types";

const boundaries = vi.hoisted(() => ({
  prepareSelection: vi.fn(() => {
    throw new Error("Bookmark lifecycle must not open playback");
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

test.each(["account", "book", "unmount"] as const)(
  "%s replacement retires a held bookmark body without publishing its old private result",
  async (replacement) => {
    const page = bookmarkPage();
    const label = "Old scope private bookmark confirmation";
    const nextLabel = "Replacement scope private unsaved draft";
    const playlist = {
      ...page.detail.playlist,
      id:
        replacement === "book"
          ? "e5652c4a-66b1-4cd7-bc45-4399f30da947"
          : page.detail.playlist.id,
      name: "Replacement private audiobook",
    };
    const next: AudiobookPageProps = {
      ...page,
      user:
        replacement === "account"
          ? { id: "bob", username: "Bob", admin: false }
          : page.user,
      playlists: [playlist],
      detail: {
        ...page.detail,
        playlist,
        bookmarks: [],
        chapters: page.detail.chapters.map((entry) => ({
          ...entry,
          title: "Replacement chapter",
        })),
      },
    };
    let batch: SyncBatch | undefined;
    let held: ReturnType<typeof heldAudiobookBody> | undefined;
    let signal: AbortSignal | undefined;
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
        batch &&
        !held
      ) {
        signal = options.signal as AbortSignal;
        held = heldAudiobookBody(bookmarkResult(page.detail, batch));
        return held.response;
      }
      throw new Error("Unexpected bookmark lifecycle request");
    });
    let view: ReturnType<typeof render> | undefined;
    try {
      view = render(createElement(AudiobookPage, page));
      expect(request).not.toHaveBeenCalled();
      enterBookmark(page, label);
      fireEvent.click(screen.getByRole("button", { name: "Save bookmark" }));
      await waitFor(() => expect(held?.response.bodyUsed).toBe(true));
      expect(signal?.aborted).toBe(false);
      expect(request).toHaveBeenCalledTimes(2);
      if (replacement === "unmount") view.unmount();
      else {
        view.rerender(createElement(AudiobookPage, next));
        const markers = screen.getByRole("region", { name: "Bookmarks" });
        expect(within(markers).queryByRole("listitem")).toBeNull();
        expect(
          (
            screen.getByRole("textbox", {
              name: "Bookmark label",
            }) as HTMLInputElement
          ).value,
        ).toBe("");
        expect(screen.queryByRole("alert")).toBeNull();
        expect(screen.queryByRole("status")).toBeNull();
        enterBookmark(next, nextLabel);
      }
      expect(signal?.aborted).toBe(true);
      await act(async () => held?.release());
      expect(screen.queryByText(label, { exact: true })).toBeNull();
      expect(screen.queryByText("Bookmark saved.", { exact: true })).toBeNull();
      expect(screen.queryByRole("alert")).toBeNull();
      if (replacement === "unmount") expect(document.body.textContent).toBe("");
      else {
        expect(
          (
            screen.getByRole("textbox", {
              name: "Bookmark label",
            }) as HTMLInputElement
          ).value,
        ).toBe(nextLabel);
        expect(
          within(screen.getByRole("region", { name: "Bookmarks" })).queryByRole(
            "listitem",
          ),
        ).toBeNull();
      }
      expect(
        request.mock.calls.map(([path, options]) => [path, options?.method]),
      ).toEqual([
        ["/api/v1/sync", "POST"],
        [`/api/v1/audiobooks/${page.detail.playlist.id}`, "GET"],
      ]);
    } finally {
      view?.unmount();
      await act(async () => held?.dispose());
    }
  },
);
