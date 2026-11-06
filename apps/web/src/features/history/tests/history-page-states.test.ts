import type { Schema } from "../../../shared/schema";
import { createElement } from "react";
import { render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import HistoryPage from "../../catalog/pages/HistoryPage";

vi.mock("../../../shared/hooks/useMutation", () => ({
  useMutation: () => ({ mutate: vi.fn() }),
}));
vi.mock("../../playback/components/PlaybackProvider", () => ({
  usePlayer: () => ({
    playQueue: vi.fn(),
    subscribe: () => () => {},
    snapshot: () => ({ current: null }),
  }),
}));
vi.mock("../../playback/components/TrackRowActions", () => ({
  default: () => null,
}));
vi.mock("../../artwork/components/Artwork", () => ({ default: () => null }));

const entry: Schema<"HistoryEntry"> = {
  id: "recording",
  title: "Fixture track",
  artist: "Fixture",
  inherited_title: "Fixture track",
  inherited_artist: "Fixture",
  duration_ms: 10000,
  position_ms: 0,
  speed: 1,
  updated_at: 1,
  available: true,
  plays_from_services: false,
  liked: false,
};

function props(
  history: Schema<"HistoryEntry">[],
  history_enabled: boolean,
): Schema<"HistoryPageProps"> {
  return {
    user: { id: "account", username: "Fixture", admin: false },
    errors: {},
    libraries: [],
    playlists: [],
    native_playback: false,
    lock: { pin_enabled: false, idle_minutes: 0, locked: false },
    activity: { history },
    history_enabled,
  };
}

test("an empty history that is recording invites the listener to start", () => {
  render(createElement(HistoryPage, props([], true)));
  expect(screen.getByRole("heading", { name: "Start listening" })).toBeTruthy();
  expect(screen.queryByText(/paused/i)).toBeNull();
});

test("an empty history that is paused explains why and links to the setting", () => {
  render(createElement(HistoryPage, props([], false)));
  expect(
    screen.getByRole("heading", { name: "Listening history is paused" }),
  ).toBeTruthy();
  expect(screen.queryByRole("heading", { name: "Start listening" })).toBeNull();
  expect(
    screen
      .getByRole("link", { name: "Open listening history settings" })
      .getAttribute("href"),
  ).toBe("/settings?section=privacy#setting-listening-history");
});

test("saved entries stay visible while paused with a notice", () => {
  render(createElement(HistoryPage, props([entry], false)));
  expect(screen.getByText("Fixture track")).toBeTruthy();
  expect(screen.getByRole("status").textContent).toContain(
    "Listening history is paused",
  );
});

test("saved entries show no paused notice while recording", () => {
  render(createElement(HistoryPage, props([entry], true)));
  expect(screen.getByText("Fixture track")).toBeTruthy();
  expect(screen.queryByRole("status")).toBeNull();
});
