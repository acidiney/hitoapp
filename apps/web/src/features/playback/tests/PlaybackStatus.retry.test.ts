import { createElement } from "react";
import { render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import PlaybackStatus from "../components/PlaybackStatus";
import type { PlaybackView } from "../model";

const playerMock = vi.hoisted(() => ({
  state: {} as Record<string, unknown>,
}));

vi.mock("../components/PlaybackProvider", () => ({
  usePlayer: () => ({
    snapshot: () => playerMock.state,
    subscribe: () => () => {},
  }),
}));

afterEach(() => {
  playerMock.state = {};
});

function view(state: Partial<PlaybackView>) {
  playerMock.state = {
    current: null,
    playing: false,
    loading: false,
    error: "",
    repeat: "off",
    shuffle: false,
    speed: 1,
    volume: 1,
    connection: "ready",
    retrying: null,
    ...state,
  } satisfies Partial<PlaybackView> as Record<string, unknown>;
}

test("a retry that is still running says so even after the player has stopped loading", () => {
  view({ loading: false, retrying: { attempt: 3, of: 3 } });
  render(createElement(PlaybackStatus));
  expect(screen.getByRole("status").textContent).toBe("Trying again (3 of 3)…");
});

test("a retry while loading says which try it is", () => {
  view({ loading: true, retrying: { attempt: 2, of: 3 } });
  render(createElement(PlaybackStatus));
  expect(screen.getByRole("status").textContent).toBe("Trying again (2 of 3)…");
});

test("once the retry ends and nothing loads there is no status line", () => {
  view({ loading: false, retrying: null });
  render(createElement(PlaybackStatus));
  expect(screen.queryByRole("status")).toBeNull();
});

test("a host that does not report retries shows the usual loading line", () => {
  view({ loading: true, retrying: undefined });
  render(createElement(PlaybackStatus));
  expect(screen.getByRole("status").textContent).toBe("Finding audio…");
});
