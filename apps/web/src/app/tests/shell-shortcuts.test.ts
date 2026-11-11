import { createElement, type ReactNode } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import ShellShortcuts from "../ShellShortcuts";

const { player, router } = vi.hoisted(() => {
  const state = { current: { id: "t1", liked: false }, volume: 0.6 };
  return {
    router: { visit: vi.fn(), reload: vi.fn() },
    player: {
      state,
      subscribe: () => () => {},
      snapshot: () => state,
      toggle: vi.fn(),
      next: vi.fn(),
      previous: vi.fn(),
      volume: vi.fn(),
      setRecordingLiked: vi.fn(),
    },
  };
});
const mutate = vi.hoisted(() => vi.fn().mockResolvedValue({ ok: true }));

vi.mock("@inertiajs/react", () => ({
  router,
  Link: ({ children, href }: { children: ReactNode; href: string }) =>
    createElement("a", { href }, children),
}));
vi.mock("../../features/playback/components/PlaybackProvider", () => ({
  usePlayer: () => player,
}));
vi.mock("../../features/playback/components/TrackActionsProvider", () => ({
  useTrackActions: () => ({ mutate, playlists: [] }),
}));

function mount(open = false, onOpenChange = vi.fn()) {
  render(createElement(ShellShortcuts, { open, onOpenChange }));
  return onOpenChange;
}

function key(init: KeyboardEventInit) {
  act(() => {
    document.body.dispatchEvent(
      new KeyboardEvent("keydown", {
        bubbles: true,
        cancelable: true,
        ...init,
      }),
    );
  });
}

beforeEach(() => {
  vi.clearAllMocks();
  window.localStorage.clear();
  player.state.volume = 0.6;
});
afterEach(cleanup);

test("space plays or pauses and Shift arrows change track", () => {
  mount();
  key({ key: " " });
  key({ key: "ArrowRight", shiftKey: true });
  key({ key: "ArrowLeft", shiftKey: true });
  expect(player.toggle).toHaveBeenCalledTimes(1);
  expect(player.next).toHaveBeenCalledTimes(1);
  expect(player.previous).toHaveBeenCalledTimes(1);
});

test("M mutes and a second M restores the volume that was playing", () => {
  mount();
  player.state.volume = 0.6;
  key({ key: "m" });
  expect(player.volume).toHaveBeenLastCalledWith(0);
  player.state.volume = 0;
  key({ key: "m" });
  expect(player.volume).toHaveBeenLastCalledWith(0.6);
});

test("L likes the playing track through the shared like action", async () => {
  mount();
  key({ key: "l" });
  await act(async () => {});
  expect(mutate).toHaveBeenCalledWith({
    kind: "like_set",
    recording_id: "t1",
    liked: true,
  });
  expect(player.setRecordingLiked).toHaveBeenCalledWith("t1", true);
});

test("G then a letter goes to Home, Library or Plugins", () => {
  mount();
  key({ key: "g" });
  key({ key: "h" });
  key({ key: "g" });
  key({ key: "l" });
  key({ key: "g" });
  key({ key: "p" });
  expect(router.visit.mock.calls.map(([path]) => path)).toEqual([
    "/home",
    "/library",
    "/settings?section=plugins",
  ]);
});

test("a question mark asks to open the list and the dialog lists what works", () => {
  const onOpenChange = mount();
  key({ key: "?", shiftKey: true });
  expect(onOpenChange).toHaveBeenCalledWith(true);
  cleanup();
  mount(true);
  expect(
    screen.getByRole("heading", { name: "Keyboard shortcuts" }),
  ).toBeTruthy();
  expect(screen.getByText("Play or pause")).toBeTruthy();
  expect(screen.getByText("G then P")).toBeTruthy();
});

test("turning single-key shortcuts off keeps them off after a reload", () => {
  mount(true);
  fireEvent.click(screen.getByRole("switch", { name: "Single-key shortcuts" }));
  expect(window.localStorage.getItem("hito.shortcuts.single")).toBe("off");
  cleanup();
  mount();
  key({ key: "m" });
  expect(player.volume).not.toHaveBeenCalled();
});
