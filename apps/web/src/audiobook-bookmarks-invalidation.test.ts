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
import AudiobookPage from "./pages/AudiobookPage";
import {
  bookmarkPage,
  bookmarkResult,
  enterBookmark,
} from "./fixtures/audiobookBookmarks";
import { heldAudiobookBody } from "./fixtures/audiobooks";
import type { AudiobookPageProps, SyncBatch } from "./types";

const boundaries = vi.hoisted(() => ({
  prepareSelection: vi.fn(() => {
    throw new Error("Bookmark invalidation must not open playback");
  }),
  visit: vi.fn(),
  reload: vi.fn(),
}));
vi.mock("./player/PlaybackProvider", () => ({
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

test("newer props retire and restart a held refresh after a first bookmark write is definitively rejected", async () => {
  const page = bookmarkPage();
  const rejected = deferredResponse();
  const first = heldAudiobookBody(page.detail);
  const newest = heldAudiobookBody({ ...page.detail, bookmarks: [] });
  const signals: AbortSignal[] = [];
  let writes = 0;
  request.mockImplementation(async (input, options) => {
    owned(options, page.user.id);
    if (input === "/api/v1/sync" && options?.method === "POST") {
      writes += 1;
      if (writes === 1) return rejected.promise;
    }
    if (
      input === `/api/v1/audiobooks/${page.detail.playlist.id}` &&
      options?.method === "GET"
    ) {
      signals.push(options.signal as AbortSignal);
      if (signals.length === 1) return first.response;
      if (signals.length === 2) return newest.response;
    }
    throw new Error("Unexpected rejected bookmark refresh request");
  });
  let view: ReturnType<typeof render> | undefined;
  try {
    view = render(createElement(AudiobookPage, page));
    expect(request).not.toHaveBeenCalled();
    enterBookmark(page, "Private rejected bookmark draft");
    fireEvent.click(screen.getByRole("button", { name: "Save bookmark" }));
    expect(request).toHaveBeenCalledTimes(1);
    view.rerender(createElement(AudiobookPage, changed(page)));
    expect(request).toHaveBeenCalledTimes(1);
    await act(async () =>
      rejected.resolve(Response.json({ error: "Rejected" }, { status: 409 })),
    );
    await waitFor(() => expect(first.response.bodyUsed).toBe(true));
    expect(signals).toHaveLength(1);
    expect(signals[0].aborted).toBe(false);
    view.rerender(createElement(AudiobookPage, changed(page)));
    expect(signals[0].aborted).toBe(true);
    await waitFor(() => expect(newest.response.bodyUsed).toBe(true));
    expect(signals).toHaveLength(2);
    expect(signals[1].aborted).toBe(false);
    await act(async () => first.release());
    expect(screen.queryByText("Bookmark saved.", { exact: true })).toBeNull();
    await act(async () => newest.release());
    const markers = screen.getByRole("region", { name: "Bookmarks" });
    await within(markers).findByText("No bookmarks yet.");
    expect(within(markers).queryByRole("listitem")).toBeNull();
    expect(
      (screen.getByRole("combobox", { name: "Chapter" }) as HTMLSelectElement)
        .disabled,
    ).toBe(false);
    expect(
      (
        screen.getByRole("button", {
          name: "Save bookmark",
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(false);
    expect(screen.queryByRole("status")).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(writes).toBe(1);
    expect(
      request.mock.calls.map(([path, options]) => [path, options?.method]),
    ).toEqual([
      ["/api/v1/sync", "POST"],
      [`/api/v1/audiobooks/${page.detail.playlist.id}`, "GET"],
      [`/api/v1/audiobooks/${page.detail.playlist.id}`, "GET"],
    ]);
  } finally {
    view?.unmount();
    rejected.resolve(Response.json({ error: "Rejected" }, { status: 409 }));
    await act(async () => {
      await first.dispose();
      await newest.dispose();
    });
  }
});

test("an obsolete mutation confirmation preserves its acknowledged operation and draft through GET-only recovery", async () => {
  const page = bookmarkPage();
  const label = "Private acknowledged bookmark awaiting a current read";
  const held: ReturnType<typeof heldAudiobookBody>[] = [];
  const savedClaims: string[] = [];
  let batch: SyncBatch | undefined;
  let reads = 0;
  let writes = 0;
  request.mockImplementation(async (input, options) => {
    owned(options, page.user.id);
    if (input === "/api/v1/sync" && options?.method === "POST") {
      writes += 1;
      if (writes !== 1)
        throw new Error("Acknowledged bookmarks must not rewrite");
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
      if (reads === 2) throw new TypeError("Private current read failure");
      if (reads === 1 || reads === 3) {
        const body = heldAudiobookBody(bookmarkResult(page.detail, batch));
        held.push(body);
        return body.response;
      }
    }
    throw new Error("Unexpected invalidated bookmark confirmation request");
  });
  let view: ReturnType<typeof render> | undefined;
  const observer = new MutationObserver((records) => {
    for (const record of records)
      for (const node of record.addedNodes)
        if (node.textContent?.includes("Bookmark saved."))
          savedClaims.push(node.textContent);
  });
  try {
    view = render(createElement(AudiobookPage, page));
    observer.observe(view.container, { childList: true, subtree: true });
    expect(request).not.toHaveBeenCalled();
    enterBookmark(page, label);
    fireEvent.click(screen.getByRole("button", { name: "Save bookmark" }));
    await waitFor(() => expect(held[0]?.response.bodyUsed).toBe(true));
    view.rerender(createElement(AudiobookPage, changed(page)));
    await act(async () => held[0].release());
    const retry = await screen.findByRole("button", {
      name: "Check bookmarks",
    });
    expect(savedClaims).toEqual([]);
    expect(screen.queryByText("Bookmark saved.", { exact: true })).toBeNull();
    expect(
      (
        screen.getByRole("textbox", {
          name: "Bookmark label",
        }) as HTMLInputElement
      ).value,
    ).toBe(label);
    expect(
      (screen.getByRole("combobox", { name: "Chapter" }) as HTMLSelectElement)
        .disabled,
    ).toBe(true);
    expect(reads).toBe(1);
    fireEvent.click(retry);
    await waitFor(() => {
      expect(reads).toBe(2);
      expect(
        (
          screen.getByRole("button", {
            name: "Check bookmarks",
          }) as HTMLButtonElement
        ).disabled,
      ).toBe(false);
    });
    expect(screen.getByRole("alert")).toBeTruthy();
    expect(writes).toBe(1);
    expect(
      screen.queryByRole("button", { name: "Retry bookmark change" }),
    ).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Check bookmarks" }));
    await waitFor(() => expect(held[1]?.response.bodyUsed).toBe(true));
    expect(savedClaims).toEqual([]);
    expect(
      within(screen.getByRole("region", { name: "Bookmarks" })).queryByText(
        label,
        { exact: true },
      ),
    ).toBeNull();
    await act(async () => held[1].release());
    await within(screen.getByRole("region", { name: "Bookmarks" })).findByText(
      label,
      { exact: true },
    );
    expect(screen.getByText("Bookmark saved.", { exact: true })).toBeTruthy();
    expect(writes).toBe(1);
    expect(
      request.mock.calls.map(([path, options]) => [path, options?.method]),
    ).toEqual([
      ["/api/v1/sync", "POST"],
      [`/api/v1/audiobooks/${page.detail.playlist.id}`, "GET"],
      [`/api/v1/audiobooks/${page.detail.playlist.id}`, "GET"],
      [`/api/v1/audiobooks/${page.detail.playlist.id}`, "GET"],
    ]);
  } finally {
    observer.disconnect();
    view?.unmount();
    await act(async () => {
      for (const body of held) await body.dispose();
    });
  }
});

function owned(options: RequestInit | undefined, account: string) {
  expect(options?.credentials).toBe("same-origin");
  expect(new Headers(options?.headers).get("x-hito-account")).toBe(account);
  expect(new Headers(options?.headers).get("x-hito-request")).toBe("1");
  expect(options?.signal).toBeInstanceOf(AbortSignal);
}

function changed(page: AudiobookPageProps): AudiobookPageProps {
  return {
    ...page,
    detail: {
      ...page.detail,
      bookmarks: page.detail.bookmarks.map((bookmark) => ({ ...bookmark })),
    },
  };
}

function deferredResponse() {
  let resolve!: (response: Response) => void;
  let settled = false;
  const promise = new Promise<Response>((done) => {
    resolve = (response) => {
      if (settled) return;
      settled = true;
      done(response);
    };
  });
  return { promise, resolve };
}
