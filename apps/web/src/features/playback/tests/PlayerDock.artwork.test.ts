import { createElement } from "react";
import { act, render } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { Playback } from "../playback";
import PlayerDock from "../components/PlayerDock";
import { ArtworkProvider } from "../../artwork/components/ArtworkProvider";

let player: Playback;
vi.mock("../components/PlaybackProvider", () => ({ usePlayer: () => player }));

test("the dock cover tries the release image before the recording image", async () => {
  const audio = document.createElement("audio");
  vi.spyOn(audio, "load").mockImplementation(() => {});
  vi.spyOn(audio, "pause").mockImplementation(() => {});
  vi.spyOn(audio, "play").mockImplementation(async () => {});
  player = new Playback(audio, async (id) => [{ id, ready: true }]);
  await act(async () => {
    await player.playQueue([
      {
        id: "dock-recording",
        release_id: "dock-release",
        title: "Fixture",
        artist: "Test",
        duration_ms: 10000,
        available: true,
      },
    ]);
  });
  const view = render(
    createElement(ArtworkProvider, {
      account: "dock-account",
      children: createElement(PlayerDock),
    }),
  );
  expect(view.container.querySelector(".dock-artwork img")?.getAttribute("src")).toBe(
    "/api/v1/releases/dock-release/artwork",
  );
  view.unmount();
});
