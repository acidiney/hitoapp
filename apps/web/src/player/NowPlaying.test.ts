import { createElement } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { Playback, type Source } from "../playback";
import PlayerDock from "./PlayerDock";

let player: Playback;
let audio: HTMLAudioElement;
let connectedAtClose: boolean[];
let resolveSources: (recording: string) => Promise<Source[]>;
vi.mock("./PlaybackProvider", () => ({ usePlayer: () => player }));
const prototype = HTMLDialogElement.prototype;
const originalShow = Object.getOwnPropertyDescriptor(prototype, "showModal");
const originalClose = Object.getOwnPropertyDescriptor(prototype, "close");

beforeEach(async () => {
  connectedAtClose = [];
  Object.defineProperties(prototype, {
    showModal: {
      configurable: true,
      value: function (this: HTMLDialogElement) {
        this.open = true;
        this.querySelector("button")?.focus();
      },
    },
    close: {
      configurable: true,
      value: function (this: HTMLDialogElement) {
        if (!this.open) return;
        connectedAtClose.push(this.isConnected);
        this.open = false;
        this.dispatchEvent(new Event("close"));
      },
    },
  });
  audio = document.createElement("audio");
  vi.spyOn(audio, "load").mockImplementation(() => {});
  vi.spyOn(audio, "pause").mockImplementation(() => {
    audio.dispatchEvent(new Event("pause"));
  });
  vi.spyOn(audio, "play").mockImplementation(async () => {
    audio.dispatchEvent(new Event("play"));
  });
  resolveSources = async (id) => [{ id, ready: true }];
  player = new Playback(audio, (id) => resolveSources(id));
  await player.playQueue([
    {
      id: "fixture",
      title: "Fixture",
      artist: "Test",
      duration_ms: 10000,
      available: true,
    },
  ]);
});

afterEach(() => {
  cleanup();
  if (originalShow) Object.defineProperty(prototype, "showModal", originalShow);
  else Reflect.deleteProperty(prototype, "showModal");
  if (originalClose) Object.defineProperty(prototype, "close", originalClose);
  else Reflect.deleteProperty(prototype, "close");
});

test.each([
  "Expand player",
  "Open now playing",
  "Expand now playing",
  "View now playing artwork",
])(
  "closing now playing restores its actual %s opener after the footer returns",
  async (label) => {
    await act(async () => {
      render(createElement(PlayerDock, { showListeningPanel: true }));
    });
    const opener = screen.getByRole("button", { name: label });
    opener.focus();
    fireEvent.click(opener);
    const dialog = screen.getByRole("dialog", { name: "Now playing" });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Close now playing" }),
    );
    expect(connectedAtClose[0]).toBe(true);
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(document.activeElement).toBe(opener);
  },
);

test("a native dialog close restores focus to the button that opened it", async () => {
  await act(async () => {
    render(createElement(PlayerDock));
  });
  const opener = screen.getByRole("button", { name: "Expand player" });
  opener.focus();
  fireEvent.click(opener);
  const dialog = screen.getByRole("dialog", { name: "Now playing" });
  if (!(dialog instanceof HTMLDialogElement)) throw new Error("Dialog missing");
  act(() => dialog.close());
  expect(document.activeElement).toBe(opener);
});

test("closing after the queue empties focuses an enabled player control", async () => {
  await act(async () => {
    render(createElement(PlayerDock));
  });
  fireEvent.click(screen.getByRole("button", { name: "Expand player" }));
  const dialog = screen.getByRole("dialog", { name: "Now playing" });
  fireEvent.click(
    await within(dialog).findByRole("button", {
      name: "Remove Fixture from queue",
    }),
  );
  fireEvent.click(
    within(dialog).getByRole("button", { name: "Close now playing" }),
  );
  expect(document.activeElement).toBe(
    screen.getByRole("button", { name: "Open queue" }),
  );
});

test("exhausted sources remain visible as a failure inside the expanded player", async () => {
  await act(async () => {
    render(createElement(PlayerDock));
  });
  fireEvent.click(screen.getByRole("button", { name: "Expand player" }));
  const dialog = screen.getByRole("dialog", { name: "Now playing" });
  resolveSources = async () => [];
  await act(async () => {
    audio.dispatchEvent(new Event("error"));
  });
  expect(within(dialog).getByRole("status").textContent).toBe(
    "Audio is unavailable for Fixture.",
  );
});

test("pending source resolution is announced inside the expanded player", async () => {
  await act(async () => {
    render(createElement(PlayerDock));
  });
  fireEvent.click(screen.getByRole("button", { name: "Expand player" }));
  const dialog = screen.getByRole("dialog", { name: "Now playing" });
  let release: ((sources: Source[]) => void) | undefined;
  resolveSources = () =>
    new Promise((resolve) => {
      release = resolve;
    });
  let opening: Promise<void> | undefined;
  act(() => {
    opening = player.playQueue([
      {
        id: "waiting",
        title: "Waiting",
        artist: "Test",
        duration_ms: 10000,
        available: true,
      },
    ]);
  });
  expect(within(dialog).getByRole("status").textContent).toBe("Finding audio…");
  await act(async () => {
    if (!release || !opening) throw new Error("Resolution was not started");
    release([{ id: "prepared", ready: true }]);
    await opening;
  });
  expect(within(dialog).queryByRole("status")).toBeNull();
});

test("disposing the player closes its native dialog and removes its opener", async () => {
  const { unmount } = await act(async () => render(createElement(PlayerDock)));
  const opener = screen.getByRole("button", { name: "Expand player" });
  fireEvent.click(opener);
  const dialog = screen.getByRole("dialog", { name: "Now playing" });
  unmount();
  expect(dialog).toHaveProperty("open", false);
  expect(opener.isConnected).toBe(false);
  expect(document.activeElement).not.toBe(opener);
});
