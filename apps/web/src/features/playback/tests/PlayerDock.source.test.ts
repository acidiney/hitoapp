import { createElement } from "react";
import { act, render } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { Playback } from "../playback";
import PlayerDock from "../components/PlayerDock";
import { ArtworkProvider } from "../../artwork/components/ArtworkProvider";

let player: Playback;
vi.mock("../components/PlaybackProvider", () => ({ usePlayer: () => player }));

test("the dock does not claim a source for the playing track", async () => {
  const audio = document.createElement("audio");
  vi.spyOn(audio, "load").mockImplementation(() => {});
  vi.spyOn(audio, "pause").mockImplementation(() => {});
  vi.spyOn(audio, "play").mockImplementation(async () => {});
  player = new Playback(audio, async (id) => [{ id, ready: true }]);
  await act(async () => {
    await player.playQueue([
      {
        id: "source-recording",
        release_id: "source-release",
        title: "Fixture",
        artist: "Test",
        duration_ms: 10000,
        available: true,
      },
    ]);
  });
  const view = render(
    createElement(ArtworkProvider, {
      account: "source-account",
      children: createElement(PlayerDock, { account: "alice" }),
    }),
  );
  expect(view.container.textContent).not.toMatch(
    /In library|Available|Catalog only/,
  );
  expect(view.container.querySelector("[data-ring]")).toBeNull();
  view.unmount();
});
