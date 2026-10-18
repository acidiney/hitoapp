// @vitest-environment jsdom
import { createElement, type ReactNode } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
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

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  vi.spyOn(console, "info").mockImplementation(() => {});
  vi.spyOn(console, "warn").mockImplementation(() => {});
  vi.spyOn(console, "debug").mockImplementation(() => {});
});
afterEach(() => {
  cleanup();
  boundaries.player?.clear();
  boundaries.player = null;
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

test("the ten-second playback deadline includes a stalled book body and permits only explicit retry", async () => {
  const { detail: page } = audiobookPages();
  const { player, resolve, progress } = audiobookPlayer();
  boundaries.player = player;
  const held = heldAudiobookBody(page.detail);
  let signal: AbortSignal | undefined;
  const request = vi.fn<typeof globalThis.fetch>(async (_, options) => {
    signal = options?.signal as AbortSignal;
    return held.response;
  });
  vi.stubGlobal("fetch", request);
  const view = render(createElement(AudiobookPage, page));
  try {
    await act(async () =>
      fireEvent.click(screen.getByRole("button", { name: "Play bookmark" })),
    );
    expect(held.response.bodyUsed).toBe(true);
    await act(async () => vi.advanceTimersByTimeAsync(9_999));
    expect(signal?.aborted).toBe(false);
    expect(screen.queryByRole("alert")).toBeNull();
    await act(async () => vi.advanceTimersByTimeAsync(1));
    expect(signal?.aborted).toBe(true);
    expect(screen.getByRole("alert").textContent).toContain(
      "Try playing again",
    );
    expect(
      (
        screen.getByRole("button", {
          name: "Play bookmark",
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(false);
    await act(async () => held.release());
    expect(resolve).not.toHaveBeenCalled();
    expect(progress).not.toHaveBeenCalled();
    expect(request).toHaveBeenCalledTimes(1);
    request.mockResolvedValue(Response.json(page.detail));
    await act(async () =>
      fireEvent.click(screen.getByRole("button", { name: "Play bookmark" })),
    );
    expect(player.snapshot().playing).toBe(true);
    expect(request).toHaveBeenCalledTimes(2);
    expect(screen.queryByRole("alert")).toBeNull();
  } finally {
    view.unmount();
    await act(async () => held.dispose());
  }
});
