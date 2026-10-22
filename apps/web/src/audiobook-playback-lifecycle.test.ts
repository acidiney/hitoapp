import { createElement, type ReactNode } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import AudiobookPage from "./pages/AudiobookPage";
import { audiobookPages, heldAudiobookBody } from "./fixtures/audiobooks";
import { audiobookPlayer } from "./fixtures/audiobookPlayback";
import type { Player } from "./player/model";

const boundaries = vi.hoisted(() => ({ player: null as Player | null }));
vi.mock("./player/PlaybackProvider", () => ({
  usePlayer: () => boundaries.player,
}));
vi.mock("@inertiajs/react", () => ({
  Link: ({ children, href }: { children: ReactNode; href: string }) =>
    createElement("a", { href }, children),
}));

const request = vi.fn<typeof globalThis.fetch>();
beforeEach(() => {
  vi.clearAllMocks();
  vi.spyOn(console, "info").mockImplementation(() => {});
  vi.spyOn(console, "warn").mockImplementation(() => {});
  vi.spyOn(console, "debug").mockImplementation(() => {});
  vi.stubGlobal("fetch", request);
});
afterEach(() => {
  cleanup();
  boundaries.player?.clear();
  boundaries.player = null;
});

test.each(["queue", "next", "clear"] as const)(
  "a held book body cannot replace a later %s playback intention",
  async (intent) => {
    const { detail: page } = audiobookPages();
    const { player, resolve } = audiobookPlayer();
    boundaries.player = player;
    const old = page.detail.chapters
      .slice(0, 2)
      .map((entry) => ({ ...entry, id: entry.recording_id! }));
    await player.playQueue(old);
    const held = heldAudiobookBody(page.detail);
    request.mockResolvedValue(held.response);
    const view = render(createElement(AudiobookPage, page));
    try {
      fireEvent.click(
        screen.getByRole("button", { name: "Play chapter 3 from beginning" }),
      );
      await waitFor(() => expect(held.response.bodyUsed).toBe(true));
      if (intent === "queue")
        await player.playQueue([{ ...old[0], id: "later-selection" }]);
      else if (intent === "next") await player.next();
      else player.clear();
      const queue = await player.queuePage(0, 10);
      const selected = player.queueSnapshot();
      const calls = resolve.mock.calls.length;
      await act(async () => held.release());
      await waitFor(() =>
        expect(
          (
            screen.getByRole("button", {
              name: "Play chapter 3 from beginning",
            }) as HTMLButtonElement
          ).disabled,
        ).toBe(false),
      );
      expect(await player.queuePage(0, 10)).toEqual(queue);
      expect(player.queueSnapshot()).toBe(selected);
      expect(resolve).toHaveBeenCalledTimes(calls);
      expect(screen.queryByRole("alert")).toBeNull();
      expect(request).toHaveBeenCalledTimes(1);
    } finally {
      view.unmount();
      await act(async () => held.dispose());
    }
  },
);

test.each(["account", "book", "props", "unmount"] as const)(
  "%s replacement aborts a held playback check and fences its late chapter result",
  async (replacement) => {
    const { detail: page } = audiobookPages();
    const { player, resolve, progress } = audiobookPlayer();
    boundaries.player = player;
    const held = heldAudiobookBody(page.detail);
    let signal: AbortSignal | undefined;
    request.mockImplementation(async (_, options) => {
      signal = options?.signal as AbortSignal;
      return held.response;
    });
    const view = render(createElement(AudiobookPage, page));
    try {
      fireEvent.click(screen.getByRole("button", { name: "Play bookmark" }));
      await waitFor(() => expect(held.response.bodyUsed).toBe(true));
      expect(signal?.aborted).toBe(false);
      if (replacement === "unmount") view.unmount();
      else
        view.rerender(
          createElement(AudiobookPage, {
            ...page,
            user:
              replacement === "account"
                ? { ...page.user, id: "bob" }
                : page.user,
            detail: {
              ...page.detail,
              playlist: {
                ...page.detail.playlist,
                id:
                  replacement === "book"
                    ? "replacement-book"
                    : page.detail.playlist.id,
              },
            },
          }),
        );
      expect(signal?.aborted).toBe(true);
      await act(async () => held.release());
      expect(resolve).not.toHaveBeenCalled();
      expect(progress).not.toHaveBeenCalled();
      expect(player.queueSnapshot().total).toBe(0);
      expect(screen.queryByRole("alert")).toBeNull();
      expect(request).toHaveBeenCalledTimes(1);
    } finally {
      view.unmount();
      await act(async () => held.dispose());
    }
  },
);
