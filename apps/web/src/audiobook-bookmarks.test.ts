// @vitest-environment jsdom
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
import { audiobookPages, heldAudiobookBody } from "./fixtures/audiobooks";
import type { Action, AudiobookDetail, SyncBatch } from "./types";

const boundaries = vi.hoisted(() => ({
  prepareSelection: vi.fn(() => {
    throw new Error("Editing bookmarks must not open playback");
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
  Link: ({
    children,
    href,
    ...attributes
  }: {
    children: ReactNode;
    href: string;
  }) => createElement("a", { href, ...attributes }, children),
}));

const device = "ab279594-a514-4b5f-a873-c0d13eb8f766";
const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const request = vi.fn<typeof globalThis.fetch>();
const audio = vi.fn();

beforeEach(() => {
  vi.clearAllMocks();
  vi.spyOn(console, "info").mockImplementation(() => {});
  vi.spyOn(console, "warn").mockImplementation(() => {});
  vi.spyOn(console, "debug").mockImplementation(() => {});
  vi.spyOn(console, "error").mockImplementation(() => {});
  localStorage.clear();
  localStorage.setItem("hito-device", device);
  audio.mockImplementation(() => {
    throw new Error("Editing bookmarks must not create audio");
  });
  vi.stubGlobal("Audio", audio);
  vi.stubGlobal("fetch", request);
});
afterEach(() => {
  cleanup();
  try {
    expect(boundaries.prepareSelection).not.toHaveBeenCalled();
    expect(audio).not.toHaveBeenCalled();
    expect(boundaries.visit).not.toHaveBeenCalled();
    expect(boundaries.reload).not.toHaveBeenCalled();
    expect(document.querySelector("audio")).toBeNull();
  } finally {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
    localStorage.clear();
  }
});

test("explicitly saving and removing a bookmark preserves its exact repeated chapter and other private markers", async () => {
  const { detail: page, bookmark, removedChapterBookmark } = audiobookPages();
  const original: AudiobookDetail = {
    ...page.detail,
    bookmarks: [bookmark, removedChapterBookmark],
  };
  const chapter = original.chapters[2];
  const label = "My private second occurrence note";
  const writes: SyncBatch[] = [];
  const held: ReturnType<typeof heldAudiobookBody>[] = [];
  let created:
    | Extract<Action, { kind: "audiobook_bookmark_create" }>
    | undefined;
  request.mockImplementation(async (input, options) => {
    expect(options?.credentials).toBe("same-origin");
    expect(new Headers(options?.headers).get("x-hito-account")).toBe(
      page.user.id,
    );
    expect(new Headers(options?.headers).get("x-hito-request")).toBe("1");
    expect(options?.signal).toBeInstanceOf(AbortSignal);
    if (input === "/api/v1/sync" && options?.method === "POST") {
      const batch = JSON.parse(String(options.body)) as SyncBatch;
      writes.push(batch);
      const action = batch.operations[0].action;
      if (writes.length === 1 && action.kind === "audiobook_bookmark_create")
        created = action;
      else if (
        writes.length !== 2 ||
        action.kind !== "audiobook_bookmark_remove"
      )
        throw new Error("Unexpected bookmark mutation");
      return Response.json({
        acknowledged: batch.operations.map((operation) => operation.id),
      });
    }
    if (
      input === `/api/v1/audiobooks/${original.playlist.id}` &&
      options?.method === "GET" &&
      created
    ) {
      const confirmed: AudiobookDetail = {
        ...original,
        bookmarks:
          writes.length === 1
            ? [
                ...original.bookmarks,
                {
                  id: created.bookmark_id,
                  audiobook_id: created.audiobook_id,
                  entry_id: created.entry_id,
                  recording_id: created.recording_id,
                  position_ms: created.position_ms,
                  label: created.label,
                },
              ]
            : original.bookmarks,
      };
      const body = heldAudiobookBody(confirmed);
      held.push(body);
      return body.response;
    }
    throw new Error("Unexpected bookmark request");
  });
  let view: ReturnType<typeof render> | undefined;
  try {
    view = render(createElement(AudiobookPage, { ...page, detail: original }));
    const markers = screen.getByRole("region", { name: "Bookmarks" });
    expect(within(markers).getAllByRole("listitem")).toHaveLength(2);
    expect(request).not.toHaveBeenCalled();
    const select = screen.getByRole("combobox", { name: "Chapter" });
    for (const [index, entry] of original.chapters.slice(0, 3).entries()) {
      expect(
        within(select)
          .getByRole("option", {
            name: `Chapter ${index + 1}: ${entry.title}`,
          })
          .getAttribute("value"),
      ).toBe(entry.entry_id);
    }
    fireEvent.change(select, { target: { value: chapter.entry_id } });
    const position = screen.getByRole("spinbutton", {
      name: "Position in seconds",
    });
    expect(position.getAttribute("step")).toBe("0.001");
    fireEvent.change(position, { target: { value: "35.5" } });
    fireEvent.change(screen.getByRole("textbox", { name: "Bookmark label" }), {
      target: { value: label },
    });
    expect(request).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Save bookmark" }));
    await waitFor(() => expect(held[0]?.response.bodyUsed).toBe(true));
    expect(writes).toHaveLength(1);
    expect(writes[0]).toEqual({
      device_id: device,
      operations: [
        {
          id: expect.stringMatching(uuid),
          action: {
            kind: "audiobook_bookmark_create",
            audiobook_id: original.playlist.id,
            bookmark_id: expect.stringMatching(uuid),
            entry_id: chapter.entry_id,
            recording_id: chapter.recording_id,
            position_ms: 35_500,
            label,
          },
        },
      ],
    });
    expect(within(markers).queryByText(label, { exact: true })).toBeNull();
    expect(within(markers).getAllByRole("listitem")).toHaveLength(2);
    await act(async () => held[0].release());
    const saved = await within(markers).findByText(label, { exact: true });
    const savedRow = saved.closest("li");
    if (!savedRow)
      throw new Error("A confirmed bookmark must remain a list entry");
    expect(
      within(savedRow).getByText("Chapter 3: Owned chapter A", { exact: true }),
    ).toBeTruthy();
    expect(
      within(savedRow).queryByText("Chapter 1: Owned chapter A", {
        exact: true,
      }),
    ).toBeNull();
    expect(within(markers).getAllByRole("listitem")).toHaveLength(3);
    fireEvent.click(
      within(savedRow).getByRole("button", { name: /Remove bookmark/i }),
    );
    await waitFor(() => expect(held[1]?.response.bodyUsed).toBe(true));
    expect(writes).toHaveLength(2);
    expect(writes[1]).toEqual({
      device_id: device,
      operations: [
        {
          id: expect.stringMatching(uuid),
          action: {
            kind: "audiobook_bookmark_remove",
            audiobook_id: original.playlist.id,
            bookmark_id: created?.bookmark_id,
          },
        },
      ],
    });
    expect(writes[1].operations[0].id).not.toBe(writes[0].operations[0].id);
    expect(within(markers).getByText(label, { exact: true })).toBeTruthy();
    await act(async () => held[1].release());
    await waitFor(() =>
      expect(within(markers).queryByText(label, { exact: true })).toBeNull(),
    );
    expect(within(markers).getAllByRole("listitem")).toHaveLength(2);
    expect(
      within(markers).getByText(bookmark.label, { exact: true }),
    ).toBeTruthy();
    const retained = within(markers)
      .getByText(removedChapterBookmark.label, { exact: true })
      .closest("li");
    if (!retained)
      throw new Error("The unavailable chapter's bookmark must remain");
    expect(
      within(retained).getByText("Chapter unavailable", { exact: true }),
    ).toBeTruthy();
    expect(
      request.mock.calls.map(([path, options]) => [path, options?.method]),
    ).toEqual([
      ["/api/v1/sync", "POST"],
      [`/api/v1/audiobooks/${original.playlist.id}`, "GET"],
      ["/api/v1/sync", "POST"],
      [`/api/v1/audiobooks/${original.playlist.id}`, "GET"],
    ]);
    const logs = JSON.stringify(
      [console.info, console.warn, console.debug, console.error].flatMap(
        (method) => vi.mocked(method).mock.calls,
      ),
    );
    for (const privateLabel of [
      label,
      bookmark.label,
      removedChapterBookmark.label,
    ])
      expect(logs.includes(privateLabel)).toBe(false);
  } finally {
    view?.unmount();
    await act(async () => {
      for (const body of held) await body.dispose();
    });
  }
});
