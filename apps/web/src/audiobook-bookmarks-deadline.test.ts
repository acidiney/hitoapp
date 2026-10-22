import { createElement, type ReactNode } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import AudiobookPage from "./pages/AudiobookPage";
import {
  bookmarkPage,
  bookmarkResult,
  enterBookmark,
} from "./fixtures/audiobookBookmarks";
import type { SyncBatch, SyncReceipt } from "./types";

const boundaries = vi.hoisted(() => ({
  prepareSelection: vi.fn(() => {
    throw new Error("Bookmark deadlines must not open playback");
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
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
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

test("a stalled sync acknowledgement body aborts at ten seconds and retains the exact explicit bookmark retry", async () => {
  const page = bookmarkPage();
  const label = "Private bookmark awaiting a stalled acknowledgement";
  const writes: string[] = [];
  const signals: AbortSignal[] = [];
  let held: ReturnType<typeof heldAcknowledgement> | undefined;
  request.mockImplementation(async (input, options) => {
    expect(options?.credentials).toBe("same-origin");
    expect(new Headers(options?.headers).get("x-hito-account")).toBe(
      page.user.id,
    );
    expect(new Headers(options?.headers).get("x-hito-request")).toBe("1");
    expect(options?.signal).toBeInstanceOf(AbortSignal);
    if (input === "/api/v1/sync" && options?.method === "POST") {
      writes.push(String(options.body));
      signals.push(options.signal as AbortSignal);
      const batch = JSON.parse(writes[0]) as SyncBatch;
      const receipt: SyncReceipt = {
        acknowledged: batch.operations.map((operation) => operation.id),
      };
      if (writes.length === 1) {
        held = heldAcknowledgement(receipt, signals[0]);
        return held.response;
      }
      if (writes.length === 2) return Response.json(receipt);
    }
    if (
      input === `/api/v1/audiobooks/${page.detail.playlist.id}` &&
      options?.method === "GET" &&
      writes.length === 2
    )
      return Response.json(
        bookmarkResult(page.detail, JSON.parse(writes[0]) as SyncBatch),
      );
    throw new Error("Unexpected bookmark deadline request");
  });
  let view: ReturnType<typeof render> | undefined;
  try {
    view = render(createElement(AudiobookPage, page));
    expect(request).not.toHaveBeenCalled();
    enterBookmark(page, label);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Save bookmark" }));
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(held?.response.bodyUsed).toBe(true);
    expect(signals).toHaveLength(1);
    expect(signals[0].aborted).toBe(false);
    await act(async () => vi.advanceTimersByTimeAsync(9_999));
    expect(signals[0].aborted).toBe(false);
    expect(screen.queryByRole("alert")).toBeNull();
    await act(async () => vi.advanceTimersByTimeAsync(1));
    expect(signals[0].aborted).toBe(true);
    const retry = screen.getByRole("button", { name: "Retry bookmark change" });
    expect(screen.getByRole("alert")).toBeTruthy();
    expect(request).toHaveBeenCalledTimes(1);
    expect(screen.queryByText("Bookmark saved.", { exact: true })).toBeNull();
    expect(
      within(screen.getByRole("region", { name: "Bookmarks" })).queryByText(
        label,
        { exact: true },
      ),
    ).toBeNull();
    expect(vi.getTimerCount()).toBe(0);
    await act(async () => vi.advanceTimersByTimeAsync(60_000));
    expect(request).toHaveBeenCalledTimes(1);
    expect(writes).toHaveLength(1);
    await act(async () => {
      fireEvent.click(retry);
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(writes).toHaveLength(2);
    expect(writes[1]).toBe(writes[0]);
    expect(signals[1].aborted).toBe(false);
    expect(
      within(screen.getByRole("region", { name: "Bookmarks" })).getByText(
        label,
        { exact: true },
      ),
    ).toBeTruthy();
    expect(screen.getByText("Bookmark saved.", { exact: true })).toBeTruthy();
    expect(
      request.mock.calls.map(([path, options]) => [path, options?.method]),
    ).toEqual([
      ["/api/v1/sync", "POST"],
      ["/api/v1/sync", "POST"],
      [`/api/v1/audiobooks/${page.detail.playlist.id}`, "GET"],
    ]);
    expect(vi.getTimerCount()).toBe(0);
    const logs = JSON.stringify(
      [console.info, console.warn, console.debug].flatMap(
        (method) => vi.mocked(method).mock.calls,
      ),
    );
    expect(logs.includes(label)).toBe(false);
  } finally {
    view?.unmount();
    await act(async () => held?.dispose());
  }
});

function heldAcknowledgement(receipt: SyncReceipt, signal: AbortSignal) {
  const bytes = new TextEncoder().encode(JSON.stringify(receipt));
  let controller!: ReadableStreamDefaultController<Uint8Array>;
  let settled = false;
  const abort = () => {
    if (settled) return;
    settled = true;
    signal.removeEventListener("abort", abort);
    controller.error(new DOMException("The request was aborted", "AbortError"));
  };
  const response = new Response(
    new ReadableStream<Uint8Array>({
      start(stream) {
        controller = stream;
        stream.enqueue(bytes.slice(0, 1));
      },
      cancel() {
        settled = true;
        signal.removeEventListener("abort", abort);
      },
    }),
    { headers: { "content-type": "application/json" } },
  );
  signal.addEventListener("abort", abort, { once: true });
  if (signal.aborted) abort();
  function release() {
    if (settled) return;
    settled = true;
    signal.removeEventListener("abort", abort);
    controller.enqueue(bytes.slice(1));
    controller.close();
  }
  return {
    response,
    release,
    async dispose() {
      release();
      if (!response.bodyUsed) await response.body?.cancel();
    },
  };
}
