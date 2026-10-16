// @vitest-environment jsdom
import { createElement } from "react";
import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { change, libraryApi } from "../api";
import { NoticeBanner, NoticeProvider } from "../components/Notices";
import type { Player } from "./model";
import type { SyncReceipt } from "../types";
import { PlaybackProvider, usePlayer } from "./PlaybackProvider";
import { nativeService } from "./native/fixtures/service";

vi.mock("../api", () => ({
  change: vi.fn(),
  libraryApi: { sources: vi.fn() },
}));

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

test("native account playback creates no browser audio and disposes old account commands and streams", async () => {
  const service = nativeService();
  const audio = vi.fn(() => {
    throw new Error("Native playback must not create browser audio");
  });
  vi.stubGlobal("Audio", audio);
  let player!: Player;
  function Capture() {
    player = usePlayer();
    return createElement("span", null, player.mode);
  }
  const view = (account: string) =>
    createElement(
      NoticeProvider,
      null,
      createElement(PlaybackProvider, {
        account,
        native: true,
        children: createElement(Capture),
      }),
    );
  const { rerender } = render(view("alice"));
  await screen.findByText("native");
  await vi.waitFor(() => expect(service.claims).toBe(1));
  const prepared = player.prepareSelection();
  const signals = vi
    .mocked(fetch)
    .mock.calls.filter(
      ([, options]) =>
        (options?.headers as Record<string, string>)?.["x-hito-account"] ===
        "alice",
    )
    .map(([, options]) => options?.signal);
  rerender(view("bob"));
  await vi.waitFor(() => expect(service.claims).toBe(2));
  expect(signals.every((signal) => signal?.aborted)).toBe(true);
  expect(
    await prepared({
      id: "late",
      title: "Private",
      artist: "Alice",
      duration_ms: 1,
      available: true,
    }),
  ).toBe(false);
  expect(audio).not.toHaveBeenCalled();
  expect(document.querySelectorAll("audio")).toHaveLength(0);
  expect(service.commands).toHaveLength(0);
});

test("account playback aborts progress and replaces its account-bound resources", async () => {
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  vi.mocked(libraryApi.sources).mockResolvedValue([
    {
      id: "source",
      recording_id: "recording",
      provider: "local",
      device_id: "server",
      ready: true,
    },
  ]);
  let failProgress!: (error: Error) => void;
  vi.mocked(change).mockReturnValueOnce(
    new Promise<SyncReceipt>((_, reject) => {
      failProgress = reject;
    }),
  );
  let player!: Player;
  function Capture() {
    player = usePlayer();
    return null;
  }
  const view = (account: string) =>
    createElement(
      NoticeProvider,
      null,
      createElement(PlaybackProvider, {
        account,
        children: createElement(Capture),
      }),
      createElement(NoticeBanner),
    );
  const { rerender } = render(view("alice"));
  const track = {
    id: "recording",
    title: "Fixture",
    artist: "Test",
    duration_ms: 20000,
    available: true,
  };
  await act(async () => {
    await player.playQueue([track]);
  });
  const audio = document.querySelector("audio");
  if (!audio) throw new Error("Audio element was not mounted");
  act(() => {
    audio.currentTime = 11;
    audio.dispatchEvent(new Event("timeupdate"));
  });
  expect(change).toHaveBeenCalledWith(
    "alice",
    expect.objectContaining({ kind: "progress_set" }),
    expect.any(AbortSignal),
  );
  const signal = vi.mocked(change).mock.calls[0][2];
  rerender(view("bob"));
  expect(signal?.aborted).toBe(true);
  await act(async () => {
    failProgress(new Error("Alice private failure"));
  });
  expect(screen.queryByText("Alice private failure")).toBeNull();
  await act(async () => {
    await player.playQueue([track]);
  });
  const latest = vi.mocked(libraryApi.sources).mock.calls.at(-1);
  expect(latest?.[0]).toBe("bob");
  expect(latest?.[2]?.aborted).toBe(false);
  expect(document.querySelectorAll("audio")).toHaveLength(1);
});
