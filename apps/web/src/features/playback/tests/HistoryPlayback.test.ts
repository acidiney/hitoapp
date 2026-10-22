import { createElement } from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import HistoryPage from "../../catalog/pages/HistoryPage";
import type { HistoryPageProps } from "../../../shared/types";

const playQueue = vi.fn(async () => {});
vi.mock("../components/PlaybackProvider", () => ({
  usePlayer: () => ({
    playQueue,
    subscribe: () => () => {},
    snapshot: () => ({ current: null }),
  }),
}));
vi.mock("../../../shared/hooks/useMutation", () => ({
  useMutation: () => ({ mutate: vi.fn() }),
}));
vi.mock("../components/TrackActions", () => ({ default: () => null }));
vi.mock("../../artwork/components/Artwork", () => ({ default: () => null }));
afterEach(() => {
  cleanup();
  playQueue.mockClear();
});

test("History row playback carries saved intent while toolbar playback starts normally", () => {
  const props: HistoryPageProps = {
    user: { id: "account", username: "Fixture", admin: false },
    errors: {},
    libraries: [],
    playlists: [],
    native_playback: true,
    activity: {
      history: [
        {
          id: "recording",
          title: "Fixture track",
          artist: "Fixture",
          inherited_title: "Fixture track",
          inherited_artist: "Fixture",
          duration_ms: 10000,
          position_ms: 2500,
          speed: 2,
          updated_at: 1,
          available: true,
          liked: false,
        },
      ],
    },
  };
  render(createElement(HistoryPage, props));
  fireEvent.click(screen.getByRole("button", { name: "Play Fixture track" }));
  expect(playQueue).toHaveBeenLastCalledWith(expect.any(Array), 0, {
    kind: "saved",
    position: 2.5,
  });
  fireEvent.click(screen.getByRole("button", { name: "Play tracks" }));
  expect(playQueue).toHaveBeenLastCalledWith(expect.any(Array));
});
