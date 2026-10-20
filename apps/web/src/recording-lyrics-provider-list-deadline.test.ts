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
import { afterEach, expect, test, vi } from "vitest";
import { lyricRecordingPage } from "./fixtures/recordingLyrics";
import {
  attachedLyricProvider,
  heldLyricJson,
  lyricProviderPage,
  primaryLyricProvider,
} from "./fixtures/recordingLyricsProviders";
import { automaticChoice } from "./fixtures/sourceChoice";
import { log } from "./log";

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ href, children }: { href: string; children: ReactNode }) =>
    createElement("a", { href }, children),
}));

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  vi.clearAllMocks();
  localStorage.clear();
  sessionStorage.clear();
});

test("the provider-list deadline includes its JSON body and fences a late list after explicit retry", async () => {
  vi.spyOn(console, "info").mockImplementation(() => {});
  const errors = vi.spyOn(console, "error");
  vi.spyOn(console, "warn").mockImplementation(() => {});
  const debug = vi.spyOn(log, "debug");
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const page = structuredClone(lyricRecordingPage);
  const before = structuredClone(page);
  const base = `/api/v1/recordings/${page.detail.recording_id}`;
  const old = heldLyricJson({
    recording_id: page.detail.recording_id,
    items: [primaryLyricProvider],
  });
  let lists = 0;
  const requests: { path: string; options?: RequestInit }[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string, options?: RequestInit) => {
      requests.push({ path, options });
      if (path === `${base}/download`)
        return Response.json({ error: "Not found" }, { status: 404 });
      if (path === `${base}/source-choice`)
        return Response.json(automaticChoice);
      if (path === `${base}/lyrics/providers`) {
        lists++;
        return lists === 1
          ? old.response
          : Response.json({
              recording_id: page.detail.recording_id,
              items: [attachedLyricProvider],
            });
      }
      throw new Error(
        "Unexpected request outside owned provider-list deadline",
      );
    }),
  );
  const view = render(lyricProviderPage(page));
  try {
    await waitFor(() => expect(requests).toHaveLength(2));
    const editor = within(
      screen.getByRole("region", { name: "Your track details" }),
    );
    const title = editor.getByRole<HTMLInputElement>("textbox", {
      name: "Track title",
    });
    fireEvent.change(title, {
      target: { value: "Unsent provider-list deadline draft" },
    });
    const lyrics = within(screen.getByRole("region", { name: "Lyrics" }));
    vi.useFakeTimers();
    await act(async () => {
      fireEvent.click(
        lyrics.getByRole("button", { name: "Choose lyric provider" }),
      );
    });
    expect(lists).toBe(1);
    expect(old.response.bodyUsed).toBe(true);
    const signal = requests[2].options!.signal!;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(4_999);
    });
    expect(signal.aborted).toBe(false);
    expect(lyrics.getByRole("status").textContent).toBe(
      "Loading lyric providers…",
    );
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1);
    });
    expect(signal.aborted).toBe(true);
    expect(lyrics.getByRole("alert").textContent).toBe(
      "Lyric providers took too long to load. Try again.",
    );
    expect(lyrics.queryByRole("combobox")).toBeNull();
    expect(lyrics.queryByRole("button", { name: "Show lyrics" })).toBeNull();
    await act(async () => {
      fireEvent.click(
        lyrics.getByRole("button", { name: "Choose lyric provider" }),
      );
    });
    expect(lists).toBe(2);
    const selector = lyrics.getByRole<HTMLSelectElement>("combobox", {
      name: "Lyric provider",
    });
    expect(selector.value).toBe(attachedLyricProvider.observation_id);
    await act(async () => {
      old.release();
    });
    expect(
      within(selector)
        .getAllByRole("option")
        .map((option) => option.textContent),
    ).toEqual([attachedLyricProvider.label]);
    expect(lyrics.queryByRole("alert")).toBeNull();
    expect(lyrics.queryByRole("status")).toBeNull();
    expect(title.value).toBe("Unsent provider-list deadline draft");
    expect(editor.getByRole("textbox", { name: "Track title" })).toBe(title);
    expect(
      requests.every((request) => (request.options?.method ?? "GET") === "GET"),
    ).toBe(true);
    expect(requests).toHaveLength(4);
    expect(page).toEqual(before);
    expect(play).not.toHaveBeenCalled();
    expect(document.querySelector("audio")?.getAttribute("src")).toBeNull();
    const completed = debug.mock.calls.filter(
      ([event]) => event === "lyrics.providers.finished",
    );
    expect(completed.map(([, fields]) => fields?.reason)).toEqual([
      "read",
      "deadline",
    ]);
    expect(
      new Set(completed.map(([, fields]) => fields?.request_id)).size,
    ).toBe(2);
    for (const [, fields] of completed) {
      expect(fields?.request_id).toMatch(
        /^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$/,
      );
      expect(fields?.duration_ms).toEqual(expect.any(Number));
    }
    view.unmount();
    expect(requests.every((request) => request.options?.signal?.aborted)).toBe(
      true,
    );
    expect(errors).not.toHaveBeenCalled();
  } finally {
    view.unmount();
    await act(async () => {
      old.release();
    });
  }
});
